// SPDX-License-Identifier: MIT OR Apache-2.0
//! Output formatting utilities for the ABP CLI.

use abp_core::{AgentEvent, AgentEventKind, ExecutionLane, Outcome, Receipt, WorkOrder};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Supported output formats for CLI display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    /// Compact JSON (single line).
    Json,
    /// Pretty-printed JSON.
    JsonPretty,
    /// Human-readable multi-line text.
    Text,
    /// Key-value aligned table.
    Table,
    /// Single-line summary.
    Compact,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Json => "json",
            Self::JsonPretty => "json-pretty",
            Self::Text => "text",
            Self::Table => "table",
            Self::Compact => "compact",
        };
        f.write_str(s)
    }
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "json-pretty" | "json_pretty" | "jsonpretty" => Ok(Self::JsonPretty),
            "text" => Ok(Self::Text),
            "table" => Ok(Self::Table),
            "compact" => Ok(Self::Compact),
            other => Err(format!("unknown output format: {other}")),
        }
    }
}

/// Formats ABP contract types for CLI output.
#[derive(Debug, Clone)]
pub struct Formatter {
    format: OutputFormat,
}

impl Formatter {
    /// Create a new formatter with the given output format.
    #[must_use]
    pub fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    /// Format a [`Receipt`] according to the configured output format.
    #[must_use]
    pub fn format_receipt(&self, receipt: &Receipt) -> String {
        match &self.format {
            OutputFormat::Json => serde_json::to_string(receipt).unwrap_or_default(),
            OutputFormat::JsonPretty => serde_json::to_string_pretty(receipt).unwrap_or_default(),
            OutputFormat::Text => format_receipt_text(receipt),
            OutputFormat::Table => format_receipt_table(receipt),
            OutputFormat::Compact => format_receipt_compact(receipt),
        }
    }

    /// Format an [`AgentEvent`] according to the configured output format.
    #[must_use]
    pub fn format_event(&self, event: &AgentEvent) -> String {
        match &self.format {
            OutputFormat::Json => serde_json::to_string(event).unwrap_or_default(),
            OutputFormat::JsonPretty => serde_json::to_string_pretty(event).unwrap_or_default(),
            OutputFormat::Text => format_event_text(event),
            OutputFormat::Table => format_event_table(event),
            OutputFormat::Compact => format_event_compact(event),
        }
    }

    /// Format a [`WorkOrder`] according to the configured output format.
    #[must_use]
    pub fn format_work_order(&self, wo: &WorkOrder) -> String {
        match &self.format {
            OutputFormat::Json => serde_json::to_string(wo).unwrap_or_default(),
            OutputFormat::JsonPretty => serde_json::to_string_pretty(wo).unwrap_or_default(),
            OutputFormat::Text => format_work_order_text(wo),
            OutputFormat::Table => format_work_order_table(wo),
            OutputFormat::Compact => format_work_order_compact(wo),
        }
    }

    /// Format an error message according to the configured output format.
    #[must_use]
    pub fn format_error(&self, err: &str) -> String {
        match &self.format {
            OutputFormat::Json | OutputFormat::JsonPretty => {
                serde_json::json!({"error": err}).to_string()
            }
            OutputFormat::Text => format!("Error: {err}"),
            OutputFormat::Table => format!("error  {err}"),
            OutputFormat::Compact => format!("[error] {err}"),
        }
    }
}

// ── Text helpers ──────────────────────────────────────────────────────

fn outcome_str(o: &Outcome) -> &'static str {
    match o {
        Outcome::Complete => "complete",
        Outcome::Partial => "partial",
        Outcome::Failed => "failed",
    }
}

fn lane_str(l: &ExecutionLane) -> &'static str {
    match l {
        ExecutionLane::PatchFirst => "patch_first",
        ExecutionLane::WorkspaceFirst => "workspace_first",
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn event_kind_tag(kind: &AgentEventKind) -> &'static str {
    match kind {
        AgentEventKind::RunStarted { .. } => "run_started",
        AgentEventKind::RunCompleted { .. } => "run_completed",
        AgentEventKind::AssistantDelta { .. } => "assistant_delta",
        AgentEventKind::AssistantMessage { .. } => "assistant_message",
        AgentEventKind::ToolCall { .. } => "tool_call",
        AgentEventKind::ToolResult { .. } => "tool_result",
        AgentEventKind::FileChanged { .. } => "file_changed",
        AgentEventKind::CommandExecuted { .. } => "command_executed",
        AgentEventKind::Warning { .. } => "warning",
        AgentEventKind::Error { .. } => "error",
    }
}

fn event_brief(kind: &AgentEventKind) -> String {
    match kind {
        AgentEventKind::RunStarted { message } => message.clone(),
        AgentEventKind::RunCompleted { message } => message.clone(),
        AgentEventKind::AssistantDelta { text } => truncate(text, 60),
        AgentEventKind::AssistantMessage { text } => truncate(text, 60),
        AgentEventKind::ToolCall { tool_name, .. } => format!("call {tool_name}"),
        AgentEventKind::ToolResult {
            tool_name,
            is_error,
            ..
        } => {
            if *is_error {
                format!("{tool_name} (error)")
            } else {
                format!("{tool_name} (ok)")
            }
        }
        AgentEventKind::FileChanged { path, summary } => format!("{path}: {summary}"),
        AgentEventKind::CommandExecuted {
            command, exit_code, ..
        } => match exit_code {
            Some(code) => format!("{} => {code}", truncate(command, 40)),
            None => truncate(command, 40),
        },
        AgentEventKind::Warning { message } => truncate(message, 60),
        AgentEventKind::Error { message } => truncate(message, 60),
    }
}

// ── Receipt formatters ────────────────────────────────────────────────

fn format_receipt_text(r: &Receipt) -> String {
    let model = r.backend.id.as_str();
    format!(
        "Outcome: {}\nBackend: {}\nDuration: {}ms\nEvents: {}",
        outcome_str(&r.outcome),
        model,
        r.meta.duration_ms,
        r.trace.len(),
    )
}

fn format_receipt_table(r: &Receipt) -> String {
    let mut lines = Vec::new();
    lines.push(format!("{:<12} {}", "outcome", outcome_str(&r.outcome)));
    lines.push(format!("{:<12} {}", "backend", r.backend.id));
    lines.push(format!("{:<12} {}ms", "duration", r.meta.duration_ms));
    lines.push(format!("{:<12} {}", "events", r.trace.len()));
    lines.push(format!("{:<12} {}", "run_id", r.meta.run_id));
    if let Some(ref hash) = r.receipt_sha256 {
        lines.push(format!("{:<12} {hash}", "sha256"));
    }
    lines.join("\n")
}

fn format_receipt_compact(r: &Receipt) -> String {
    format!(
        "[{}] backend={} duration={}ms events={}",
        outcome_str(&r.outcome),
        r.backend.id,
        r.meta.duration_ms,
        r.trace.len(),
    )
}

// ── Event formatters ──────────────────────────────────────────────────

fn format_event_text(ev: &AgentEvent) -> String {
    let ts = ev.ts.format("%H:%M:%S%.3f");
    let tag = event_kind_tag(&ev.kind);
    let brief = event_brief(&ev.kind);
    format!("[{ts}] {tag}: {brief}")
}

fn format_event_table(ev: &AgentEvent) -> String {
    let ts = ev.ts.format("%H:%M:%S%.3f");
    let tag = event_kind_tag(&ev.kind);
    let brief = event_brief(&ev.kind);
    format!("{:<16} {:<20} {}", ts, tag, brief)
}

fn format_event_compact(ev: &AgentEvent) -> String {
    let tag = event_kind_tag(&ev.kind);
    let brief = event_brief(&ev.kind);
    format!("[{tag}] {brief}")
}

// ── WorkOrder formatters ──────────────────────────────────────────────

fn format_work_order_text(wo: &WorkOrder) -> String {
    format!(
        "ID: {}\nTask: {}\nLane: {}",
        wo.id,
        truncate(&wo.task, 80),
        lane_str(&wo.lane),
    )
}

fn format_work_order_table(wo: &WorkOrder) -> String {
    let mut lines = Vec::new();
    lines.push(format!("{:<12} {}", "id", wo.id));
    lines.push(format!("{:<12} {}", "task", truncate(&wo.task, 80)));
    lines.push(format!("{:<12} {}", "lane", lane_str(&wo.lane)));
    lines.push(format!("{:<12} {}", "root", wo.workspace.root));
    if let Some(ref model) = wo.config.model {
        lines.push(format!("{:<12} {model}", "model"));
    }
    lines.join("\n")
}

fn format_work_order_compact(wo: &WorkOrder) -> String {
    format!(
        "[{}] {} lane={}",
        wo.id,
        truncate(&wo.task, 50),
        lane_str(&wo.lane),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_display_roundtrips() {
        for fmt in &[
            OutputFormat::Json,
            OutputFormat::JsonPretty,
            OutputFormat::Text,
            OutputFormat::Table,
            OutputFormat::Compact,
        ] {
            let s = fmt.to_string();
            let parsed: OutputFormat = s.parse().unwrap();
            assert_eq!(&parsed, fmt);
        }
    }

    #[test]
    fn output_format_from_str_rejects_unknown() {
        assert!("nope".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let long = "a".repeat(100);
        let t = truncate(&long, 10);
        assert!(t.len() < 100);
        assert!(t.ends_with('…'));
    }
}
