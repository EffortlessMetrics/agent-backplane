// SPDX-License-Identifier: MIT OR Apache-2.0
//! Projection matrix for translating between vendor dialects.
//!
//! In v0.1, the matrix supports:
//! - **Identity translations**: same dialect in and out (pass-through).
//! - **ABP-to-vendor translations**: convert an ABP [`WorkOrder`] into the
//!   vendor-specific request JSON for each supported dialect.

use abp_core::{AgentEvent, AgentEventKind, WorkOrder};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

/// Identifies a vendor dialect for translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    /// The canonical ABP contract format.
    Abp,
    /// Anthropic Claude Messages API.
    Claude,
    /// OpenAI Codex / Responses API.
    Codex,
    /// Google Gemini generateContent API.
    Gemini,
    /// Moonshot Kimi chat completions API.
    Kimi,
}

impl Dialect {
    /// All known dialect variants.
    pub const ALL: &[Dialect] = &[
        Dialect::Abp,
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
    ];
}

/// Maps tool names between two dialects.
#[derive(Debug, Clone, Default)]
pub struct ToolTranslation {
    /// Source tool name → target tool name.
    pub name_map: HashMap<String, String>,
}

/// Maps event kind names between two dialects.
#[derive(Debug, Clone, Default)]
pub struct EventMapping {
    /// Source event kind → target event kind.
    pub kind_map: HashMap<String, String>,
}

/// Standalone tool call representation for dialect translation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool name.
    pub tool_name: String,
    /// Unique identifier for this tool invocation.
    pub tool_use_id: Option<String>,
    /// Parent tool use ID for nested calls.
    pub parent_tool_use_id: Option<String>,
    /// Input arguments.
    pub input: serde_json::Value,
}

/// Standalone tool result representation for dialect translation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// Tool name.
    pub tool_name: String,
    /// Unique identifier matching the tool call.
    pub tool_use_id: Option<String>,
    /// Output value.
    pub output: serde_json::Value,
    /// Whether the tool execution resulted in an error.
    pub is_error: bool,
}

/// Routes translations between vendor dialects.
///
/// The projection matrix knows which `(source, target)` dialect pairs are
/// valid and performs the mapping via inline translation logic that mirrors
/// each SDK adapter's `map_work_order` function.
///
/// Supports two layers of translation:
/// - **WorkOrder translation** via [`Dialect`] enum (ABP → vendor request JSON)
/// - **Tool/event translation** via string dialect names (`"abp"`, `"openai"`,
///   `"anthropic"`, `"gemini"`) for tool call/result and event mapping.
#[derive(Debug, Clone)]
pub struct ProjectionMatrix {
    tool_translations: HashMap<(String, String), ToolTranslation>,
    event_mappings: HashMap<(String, String), EventMapping>,
    registered_dialects: Vec<String>,
}

impl Default for ProjectionMatrix {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectionMatrix {
    /// Create a new projection matrix with built-in dialect registrations.
    #[must_use]
    pub fn new() -> Self {
        let mut matrix = Self {
            tool_translations: HashMap::new(),
            event_mappings: HashMap::new(),
            registered_dialects: vec![
                "abp".into(),
                "openai".into(),
                "anthropic".into(),
                "gemini".into(),
            ],
        };
        matrix.register_builtin_translations();
        matrix
    }

    /// Translate a [`WorkOrder`] from one dialect to another.
    ///
    /// Identity translations serialise the work order as-is.
    /// ABP-to-vendor translations build the vendor request JSON using
    /// the same logic as each SDK adapter's `map_work_order`.
    ///
    /// # Errors
    ///
    /// Returns an error when the `(from, to)` pair is not a supported
    /// translation in the current version of the matrix.
    pub fn translate(
        &self,
        from: Dialect,
        to: Dialect,
        wo: &WorkOrder,
    ) -> Result<serde_json::Value> {
        translate(from, to, wo)
    }

    /// List all WorkOrder translation pairs the matrix currently supports.
    #[must_use]
    pub fn supported_translations(&self) -> Vec<(Dialect, Dialect)> {
        supported_translations()
    }

    // -----------------------------------------------------------------
    // Tool call / result translation (string-based dialect names)
    // -----------------------------------------------------------------

    /// Translate a [`ToolCall`] from one dialect to another.
    ///
    /// Tool names are mapped according to the built-in translation tables.
    /// Names without an explicit mapping are passed through unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error if either dialect name is unknown.
    pub fn translate_tool_call(&self, from: &str, to: &str, call: &ToolCall) -> Result<ToolCall> {
        self.validate_dialects(from, to)?;
        if from == to {
            return Ok(call.clone());
        }
        let key = (from.to_string(), to.to_string());
        let translated_name = self
            .tool_translations
            .get(&key)
            .and_then(|t| t.name_map.get(&call.tool_name))
            .cloned()
            .unwrap_or_else(|| call.tool_name.clone());
        Ok(ToolCall {
            tool_name: translated_name,
            tool_use_id: call.tool_use_id.clone(),
            parent_tool_use_id: call.parent_tool_use_id.clone(),
            input: call.input.clone(),
        })
    }

    /// Translate a [`ToolResult`] from one dialect to another.
    ///
    /// # Errors
    ///
    /// Returns an error if either dialect name is unknown.
    pub fn translate_tool_result(
        &self,
        from: &str,
        to: &str,
        result: &ToolResult,
    ) -> Result<ToolResult> {
        self.validate_dialects(from, to)?;
        if from == to {
            return Ok(result.clone());
        }
        let key = (from.to_string(), to.to_string());
        let translated_name = self
            .tool_translations
            .get(&key)
            .and_then(|t| t.name_map.get(&result.tool_name))
            .cloned()
            .unwrap_or_else(|| result.tool_name.clone());
        Ok(ToolResult {
            tool_name: translated_name,
            tool_use_id: result.tool_use_id.clone(),
            output: result.output.clone(),
            is_error: result.is_error,
        })
    }

    /// Translate an [`AgentEvent`] from one dialect to another.
    ///
    /// For events containing tool calls or results, the tool name is
    /// translated. Other event kinds are passed through unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error if either dialect name is unknown.
    pub fn translate_event(&self, from: &str, to: &str, event: &AgentEvent) -> Result<AgentEvent> {
        self.validate_dialects(from, to)?;
        if from == to {
            return Ok(event.clone());
        }
        let key = (from.to_string(), to.to_string());
        let translated_kind = match &event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                parent_tool_use_id,
                input,
            } => {
                let name = self
                    .tool_translations
                    .get(&key)
                    .and_then(|t| t.name_map.get(tool_name))
                    .cloned()
                    .unwrap_or_else(|| tool_name.clone());
                AgentEventKind::ToolCall {
                    tool_name: name,
                    tool_use_id: tool_use_id.clone(),
                    parent_tool_use_id: parent_tool_use_id.clone(),
                    input: input.clone(),
                }
            }
            AgentEventKind::ToolResult {
                tool_name,
                tool_use_id,
                output,
                is_error,
            } => {
                let name = self
                    .tool_translations
                    .get(&key)
                    .and_then(|t| t.name_map.get(tool_name))
                    .cloned()
                    .unwrap_or_else(|| tool_name.clone());
                AgentEventKind::ToolResult {
                    tool_name: name,
                    tool_use_id: tool_use_id.clone(),
                    output: output.clone(),
                    is_error: *is_error,
                }
            }
            other => other.clone(),
        };
        Ok(AgentEvent {
            ts: event.ts,
            kind: translated_kind,
            ext: event.ext.clone(),
        })
    }

    /// Return the list of registered dialect names.
    #[must_use]
    pub fn supported_dialects(&self) -> Vec<String> {
        self.registered_dialects.clone()
    }

    /// Check whether a translation path exists between two dialects.
    #[must_use]
    pub fn has_translation(&self, from: &str, to: &str) -> bool {
        if from == to {
            return self.registered_dialects.iter().any(|d| d == from);
        }
        let key = (from.to_string(), to.to_string());
        self.tool_translations.contains_key(&key) || self.event_mappings.contains_key(&key)
    }

    /// Get the tool translation table for a dialect pair.
    #[must_use]
    pub fn tool_translation(&self, from: &str, to: &str) -> Option<&ToolTranslation> {
        let key = (from.to_string(), to.to_string());
        self.tool_translations.get(&key)
    }

    /// Get the event mapping table for a dialect pair.
    #[must_use]
    pub fn event_mapping(&self, from: &str, to: &str) -> Option<&EventMapping> {
        let key = (from.to_string(), to.to_string());
        self.event_mappings.get(&key)
    }

    // -----------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------

    fn validate_dialects(&self, from: &str, to: &str) -> Result<()> {
        if !self.registered_dialects.iter().any(|d| d == from) {
            bail!("unknown dialect: {from}");
        }
        if !self.registered_dialects.iter().any(|d| d == to) {
            bail!("unknown dialect: {to}");
        }
        Ok(())
    }

    fn register_tool_translation(&mut self, from: &str, to: &str, mappings: &[(&str, &str)]) {
        let translation = ToolTranslation {
            name_map: mappings
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        };
        self.tool_translations
            .insert((from.to_string(), to.to_string()), translation);
    }

    fn register_event_mapping(&mut self, from: &str, to: &str, mappings: &[(&str, &str)]) {
        let mapping = EventMapping {
            kind_map: mappings
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        };
        self.event_mappings
            .insert((from.to_string(), to.to_string()), mapping);
    }

    #[allow(clippy::too_many_lines)]
    fn register_builtin_translations(&mut self) {
        // ── Tool name mappings ──────────────────────────────────────

        // ABP ↔ OpenAI
        self.register_tool_translation(
            "abp",
            "openai",
            &[
                ("read_file", "file_read"),
                ("write_file", "file_write"),
                ("bash", "shell"),
                ("edit_file", "apply_diff"),
                ("glob", "file_search"),
            ],
        );
        self.register_tool_translation(
            "openai",
            "abp",
            &[
                ("file_read", "read_file"),
                ("file_write", "write_file"),
                ("shell", "bash"),
                ("apply_diff", "edit_file"),
                ("file_search", "glob"),
            ],
        );

        // ABP ↔ Anthropic
        self.register_tool_translation(
            "abp",
            "anthropic",
            &[
                ("read_file", "Read"),
                ("write_file", "Write"),
                ("bash", "Bash"),
                ("edit_file", "Edit"),
                ("glob", "Glob"),
            ],
        );
        self.register_tool_translation(
            "anthropic",
            "abp",
            &[
                ("Read", "read_file"),
                ("Write", "write_file"),
                ("Bash", "bash"),
                ("Edit", "edit_file"),
                ("Glob", "glob"),
            ],
        );

        // ABP ↔ Gemini
        self.register_tool_translation(
            "abp",
            "gemini",
            &[
                ("read_file", "readFile"),
                ("write_file", "writeFile"),
                ("bash", "executeCommand"),
                ("edit_file", "editFile"),
                ("glob", "searchFiles"),
            ],
        );
        self.register_tool_translation(
            "gemini",
            "abp",
            &[
                ("readFile", "read_file"),
                ("writeFile", "write_file"),
                ("executeCommand", "bash"),
                ("editFile", "edit_file"),
                ("searchFiles", "glob"),
            ],
        );

        // OpenAI ↔ Anthropic
        self.register_tool_translation(
            "openai",
            "anthropic",
            &[
                ("file_read", "Read"),
                ("file_write", "Write"),
                ("shell", "Bash"),
                ("apply_diff", "Edit"),
                ("file_search", "Glob"),
            ],
        );
        self.register_tool_translation(
            "anthropic",
            "openai",
            &[
                ("Read", "file_read"),
                ("Write", "file_write"),
                ("Bash", "shell"),
                ("Edit", "apply_diff"),
                ("Glob", "file_search"),
            ],
        );

        // OpenAI ↔ Gemini
        self.register_tool_translation(
            "openai",
            "gemini",
            &[
                ("file_read", "readFile"),
                ("file_write", "writeFile"),
                ("shell", "executeCommand"),
                ("apply_diff", "editFile"),
                ("file_search", "searchFiles"),
            ],
        );
        self.register_tool_translation(
            "gemini",
            "openai",
            &[
                ("readFile", "file_read"),
                ("writeFile", "file_write"),
                ("executeCommand", "shell"),
                ("editFile", "apply_diff"),
                ("searchFiles", "file_search"),
            ],
        );

        // Anthropic ↔ Gemini
        self.register_tool_translation(
            "anthropic",
            "gemini",
            &[
                ("Read", "readFile"),
                ("Write", "writeFile"),
                ("Bash", "executeCommand"),
                ("Edit", "editFile"),
                ("Glob", "searchFiles"),
            ],
        );
        self.register_tool_translation(
            "gemini",
            "anthropic",
            &[
                ("readFile", "Read"),
                ("writeFile", "Write"),
                ("executeCommand", "Bash"),
                ("editFile", "Edit"),
                ("searchFiles", "Glob"),
            ],
        );

        // ── Event kind mappings ─────────────────────────────────────

        // ABP ↔ OpenAI
        self.register_event_mapping(
            "abp",
            "openai",
            &[
                ("run_started", "response.created"),
                ("run_completed", "response.completed"),
                ("assistant_message", "response.output_text.done"),
                ("assistant_delta", "response.output_text.delta"),
                ("tool_call", "function_call"),
                ("tool_result", "function_call_output"),
            ],
        );
        self.register_event_mapping(
            "openai",
            "abp",
            &[
                ("response.created", "run_started"),
                ("response.completed", "run_completed"),
                ("response.output_text.done", "assistant_message"),
                ("response.output_text.delta", "assistant_delta"),
                ("function_call", "tool_call"),
                ("function_call_output", "tool_result"),
            ],
        );

        // ABP ↔ Anthropic
        self.register_event_mapping(
            "abp",
            "anthropic",
            &[
                ("run_started", "message_start"),
                ("run_completed", "message_stop"),
                ("assistant_message", "content_block_stop"),
                ("assistant_delta", "content_block_delta"),
                ("tool_call", "tool_use"),
                ("tool_result", "tool_result"),
            ],
        );
        self.register_event_mapping(
            "anthropic",
            "abp",
            &[
                ("message_start", "run_started"),
                ("message_stop", "run_completed"),
                ("content_block_stop", "assistant_message"),
                ("content_block_delta", "assistant_delta"),
                ("tool_use", "tool_call"),
                ("tool_result", "tool_result"),
            ],
        );

        // ABP ↔ Gemini
        self.register_event_mapping(
            "abp",
            "gemini",
            &[
                ("run_started", "generate_content_start"),
                ("run_completed", "generate_content_end"),
                ("assistant_message", "text"),
                ("assistant_delta", "text_delta"),
                ("tool_call", "function_call"),
                ("tool_result", "function_response"),
            ],
        );
        self.register_event_mapping(
            "gemini",
            "abp",
            &[
                ("generate_content_start", "run_started"),
                ("generate_content_end", "run_completed"),
                ("text", "assistant_message"),
                ("text_delta", "assistant_delta"),
                ("function_call", "tool_call"),
                ("function_response", "tool_result"),
            ],
        );

        // OpenAI ↔ Anthropic
        self.register_event_mapping(
            "openai",
            "anthropic",
            &[
                ("response.created", "message_start"),
                ("response.completed", "message_stop"),
                ("response.output_text.done", "content_block_stop"),
                ("response.output_text.delta", "content_block_delta"),
                ("function_call", "tool_use"),
                ("function_call_output", "tool_result"),
            ],
        );
        self.register_event_mapping(
            "anthropic",
            "openai",
            &[
                ("message_start", "response.created"),
                ("message_stop", "response.completed"),
                ("content_block_stop", "response.output_text.done"),
                ("content_block_delta", "response.output_text.delta"),
                ("tool_use", "function_call"),
                ("tool_result", "function_call_output"),
            ],
        );

        // OpenAI ↔ Gemini
        self.register_event_mapping(
            "openai",
            "gemini",
            &[
                ("response.created", "generate_content_start"),
                ("response.completed", "generate_content_end"),
                ("response.output_text.done", "text"),
                ("response.output_text.delta", "text_delta"),
                ("function_call", "function_call"),
                ("function_call_output", "function_response"),
            ],
        );
        self.register_event_mapping(
            "gemini",
            "openai",
            &[
                ("generate_content_start", "response.created"),
                ("generate_content_end", "response.completed"),
                ("text", "response.output_text.done"),
                ("text_delta", "response.output_text.delta"),
                ("function_call", "function_call"),
                ("function_response", "function_call_output"),
            ],
        );

        // Anthropic ↔ Gemini
        self.register_event_mapping(
            "anthropic",
            "gemini",
            &[
                ("message_start", "generate_content_start"),
                ("message_stop", "generate_content_end"),
                ("content_block_stop", "text"),
                ("content_block_delta", "text_delta"),
                ("tool_use", "function_call"),
                ("tool_result", "function_response"),
            ],
        );
        self.register_event_mapping(
            "gemini",
            "anthropic",
            &[
                ("generate_content_start", "message_start"),
                ("generate_content_end", "message_stop"),
                ("text", "content_block_stop"),
                ("text_delta", "content_block_delta"),
                ("function_call", "tool_use"),
                ("function_response", "tool_result"),
            ],
        );
    }
}

// ---------------------------------------------------------------------------
// Inline translation helpers
//
// These mirror the SDK adapter `map_work_order` functions but produce
// `serde_json::Value` directly so we avoid a cyclic dependency between
// `abp-integrations` and the `abp-*-sdk` crates.
// ---------------------------------------------------------------------------

fn build_user_content(wo: &WorkOrder) -> String {
    let mut content = wo.task.clone();
    for snippet in &wo.context.snippets {
        content.push_str(&format!(
            "\n\n--- {} ---\n{}",
            snippet.name, snippet.content
        ));
    }
    content
}

fn model_or_default<'a>(wo: &'a WorkOrder, fallback: &'a str) -> &'a str {
    wo.config.model.as_deref().unwrap_or(fallback)
}

fn wo_to_claude(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "claude-sonnet-4-20250514"),
        "max_tokens": 4096,
        "system": null,
        "messages": [{
            "role": "user",
            "content": build_user_content(wo),
        }],
    })
}

fn wo_to_codex(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "codex-mini-latest"),
        "input": [{
            "type": "message",
            "role": "user",
            "content": build_user_content(wo),
        }],
        "max_output_tokens": 4096,
    })
}

fn wo_to_gemini(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "gemini-2.5-flash"),
        "contents": [{
            "role": "user",
            "parts": [{ "Text": build_user_content(wo) }],
        }],
        "generation_config": {
            "maxOutputTokens": 4096,
        },
    })
}

fn wo_to_kimi(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "moonshot-v1-8k"),
        "messages": [{
            "role": "user",
            "content": build_user_content(wo),
        }],
        "max_tokens": 4096,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Translate a [`WorkOrder`] from one dialect to another.
///
/// Free-function form of [`ProjectionMatrix::translate`].
pub fn translate(from: Dialect, to: Dialect, wo: &WorkOrder) -> Result<serde_json::Value> {
    // Identity: same dialect in and out.
    if from == to {
        return Ok(serde_json::to_value(wo)?);
    }

    // ABP → vendor translations.
    if from == Dialect::Abp {
        return Ok(match to {
            Dialect::Claude => wo_to_claude(wo),
            Dialect::Codex => wo_to_codex(wo),
            Dialect::Gemini => wo_to_gemini(wo),
            Dialect::Kimi => wo_to_kimi(wo),
            Dialect::Abp => unreachable!("handled by identity branch"),
        });
    }

    bail!(
        "unsupported translation: {:?} -> {:?} (v0.1 supports identity and ABP-to-vendor only)",
        from,
        to
    )
}

/// List all translation pairs the matrix currently supports.
pub fn supported_translations() -> Vec<(Dialect, Dialect)> {
    let mut pairs = Vec::new();

    // Identity pairs.
    for &d in Dialect::ALL {
        pairs.push((d, d));
    }

    // ABP → each vendor dialect.
    for &d in Dialect::ALL {
        if d != Dialect::Abp {
            pairs.push((Dialect::Abp, d));
        }
    }

    pairs
}
