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
    /// Mock backend for testing and development.
    Mock,
    /// OpenAI Chat Completions API.
    #[serde(rename = "openai")]
    OpenAi,
}

impl Dialect {
    /// All known dialect variants.
    pub const ALL: &[Dialect] = &[
        Dialect::Abp,
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
        Dialect::Mock,
        Dialect::OpenAi,
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

/// Describes the expected fidelity of translating between two dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationFidelity {
    /// Lossless passthrough — no information is lost.
    Lossless,
    /// Mapped translation with known, documented information loss.
    LossySupported,
    /// Emulation layer with significant degradation.
    Degraded,
    /// Translation is not possible.
    Unsupported,
}

/// Role of a message participant in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    /// System-level instructions.
    System,
    /// User input.
    User,
    /// Assistant response.
    Assistant,
}

/// Intermediate representation of a conversation message for cross-dialect translation.
///
/// Normalizes role and content across vendor APIs so that messages can be
/// projected from one dialect to another through the ABP IR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender.
    pub role: MessageRole,
    /// The text content of the message.
    pub content: String,
}

/// Intermediate representation of a tool definition for cross-dialect translation.
///
/// Normalizes the different vendor-specific tool definition formats into a
/// common structure that can be projected into any target dialect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinitionIr {
    /// The tool name in the source dialect.
    pub name: String,
    /// Human-readable description of the tool.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub parameters: serde_json::Value,
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
                "codex".into(),
                "kimi".into(),
                "mock".into(),
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

    /// Query the translation fidelity between two dialects.
    ///
    /// Returns [`TranslationFidelity::Lossless`] for identity translations,
    /// [`TranslationFidelity::LossySupported`] for ABP-to-vendor, vendor-to-ABP,
    /// or Mock translations, [`TranslationFidelity::Degraded`] for vendor-to-vendor
    /// translations that go through the ABP IR, and
    /// [`TranslationFidelity::Unsupported`] for unknown dialect pairs.
    #[must_use]
    pub fn can_translate(&self, from: Dialect, to: Dialect) -> TranslationFidelity {
        if from == to {
            return TranslationFidelity::Lossless;
        }
        if from == Dialect::Mock || to == Dialect::Mock {
            return TranslationFidelity::LossySupported;
        }
        if from == Dialect::Abp || to == Dialect::Abp {
            return TranslationFidelity::LossySupported;
        }
        // Vendor-to-vendor: check if we have translation tables
        let from_str = dialect_to_str(from);
        let to_str = dialect_to_str(to);
        if self.has_translation(from_str, to_str) {
            return TranslationFidelity::Degraded;
        }
        TranslationFidelity::Unsupported
    }

    /// Map messages from one dialect's role conventions to another.
    ///
    /// Claude and Gemini do not support a `System` role natively — system
    /// messages are folded into user messages with a `[System]` prefix when
    /// targeting those dialects.
    ///
    /// # Errors
    ///
    /// Returns an error if the mapping cannot be performed.
    pub fn map_messages(
        &self,
        _from: Dialect,
        to: Dialect,
        messages: &[Message],
    ) -> Result<Vec<Message>> {
        let mut result = Vec::with_capacity(messages.len());
        for msg in messages {
            let mapped = match (to, msg.role) {
                // Claude and Gemini lack a native system role.
                (Dialect::Claude | Dialect::Gemini, MessageRole::System) => Message {
                    role: MessageRole::User,
                    content: format!("[System] {}", msg.content),
                },
                _ => msg.clone(),
            };
            result.push(mapped);
        }
        Ok(result)
    }

    /// Map tool definitions from one dialect to another.
    ///
    /// Tool names are translated according to the built-in translation tables.
    /// Descriptions and parameter schemas are preserved as-is since JSON Schema
    /// is compatible across all supported dialects.
    ///
    /// # Errors
    ///
    /// Returns an error if translation lookup fails.
    pub fn map_tool_definitions(
        &self,
        from: Dialect,
        to: Dialect,
        tools: &[ToolDefinitionIr],
    ) -> Result<Vec<ToolDefinitionIr>> {
        if from == to {
            return Ok(tools.to_vec());
        }
        let from_str = dialect_to_str(from);
        let to_str = dialect_to_str(to);
        let key = (from_str.to_string(), to_str.to_string());
        let mut result = Vec::with_capacity(tools.len());
        for tool in tools {
            let translated_name = self
                .tool_translations
                .get(&key)
                .and_then(|t| t.name_map.get(&tool.name))
                .cloned()
                .unwrap_or_else(|| tool.name.clone());
            result.push(ToolDefinitionIr {
                name: translated_name,
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            });
        }
        Ok(result)
    }

    /// Map a model name from one dialect to another.
    ///
    /// If the model already belongs to the target dialect it is returned as-is.
    /// Otherwise the built-in equivalence table is consulted. Abp and Mock
    /// targets always return the input model unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error if no known mapping exists for the model to the target
    /// dialect. This fails early rather than silently degrading.
    pub fn map_model_name(&self, _from: Dialect, to: Dialect, model: &str) -> Result<String> {
        if to == Dialect::Abp || to == Dialect::Mock {
            return Ok(model.to_string());
        }
        if model_belongs_to(model, to) {
            return Ok(model.to_string());
        }
        if let Some(equivalent) = find_equivalent_model(model, to) {
            return Ok(equivalent.to_string());
        }
        bail!("no known mapping for model {model:?} to dialect {to:?}")
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

        // Register translations for codex, kimi, and mock dialects.
        self.register_cross_dialect_translations();
    }

    #[allow(clippy::too_many_lines)]
    fn register_cross_dialect_translations(&mut self) {
        // Tool names per dialect, keyed by ABP canonical name.
        let tool_dialects: &[(&str, &[(&str, &str)])] = &[
            (
                "abp",
                &[
                    ("read_file", "read_file"),
                    ("write_file", "write_file"),
                    ("bash", "bash"),
                    ("edit_file", "edit_file"),
                    ("glob", "glob"),
                ],
            ),
            (
                "openai",
                &[
                    ("read_file", "file_read"),
                    ("write_file", "file_write"),
                    ("bash", "shell"),
                    ("edit_file", "apply_diff"),
                    ("glob", "file_search"),
                ],
            ),
            (
                "anthropic",
                &[
                    ("read_file", "Read"),
                    ("write_file", "Write"),
                    ("bash", "Bash"),
                    ("edit_file", "Edit"),
                    ("glob", "Glob"),
                ],
            ),
            (
                "gemini",
                &[
                    ("read_file", "readFile"),
                    ("write_file", "writeFile"),
                    ("bash", "executeCommand"),
                    ("edit_file", "editFile"),
                    ("glob", "searchFiles"),
                ],
            ),
            (
                "codex",
                &[
                    ("read_file", "file_read"),
                    ("write_file", "file_write"),
                    ("bash", "shell"),
                    ("edit_file", "apply_diff"),
                    ("glob", "file_search"),
                ],
            ),
            (
                "kimi",
                &[
                    ("read_file", "read_file"),
                    ("write_file", "write_file"),
                    ("bash", "bash"),
                    ("edit_file", "edit_file"),
                    ("glob", "glob"),
                ],
            ),
            (
                "mock",
                &[
                    ("read_file", "read_file"),
                    ("write_file", "write_file"),
                    ("bash", "bash"),
                    ("edit_file", "edit_file"),
                    ("glob", "glob"),
                ],
            ),
        ];

        for (from_name, from_tools) in tool_dialects {
            for (to_name, to_tools) in tool_dialects {
                if from_name == to_name {
                    continue;
                }
                let key = ((*from_name).to_string(), (*to_name).to_string());
                if self.tool_translations.contains_key(&key) {
                    continue; // already registered by manual setup
                }
                let mappings: Vec<(&str, &str)> = from_tools
                    .iter()
                    .filter_map(|(abp_name, from_tool)| {
                        to_tools
                            .iter()
                            .find(|(abp_n, _)| abp_n == abp_name)
                            .map(|(_, to_tool)| (*from_tool, *to_tool))
                    })
                    .filter(|(f, t)| f != t)
                    .collect();
                self.register_tool_translation(from_name, to_name, &mappings);
            }
        }

        // Event names per dialect, keyed by ABP canonical name.
        let event_dialects: &[(&str, &[(&str, &str)])] = &[
            (
                "abp",
                &[
                    ("run_started", "run_started"),
                    ("run_completed", "run_completed"),
                    ("assistant_message", "assistant_message"),
                    ("assistant_delta", "assistant_delta"),
                    ("tool_call", "tool_call"),
                    ("tool_result", "tool_result"),
                ],
            ),
            (
                "openai",
                &[
                    ("run_started", "response.created"),
                    ("run_completed", "response.completed"),
                    ("assistant_message", "response.output_text.done"),
                    ("assistant_delta", "response.output_text.delta"),
                    ("tool_call", "function_call"),
                    ("tool_result", "function_call_output"),
                ],
            ),
            (
                "anthropic",
                &[
                    ("run_started", "message_start"),
                    ("run_completed", "message_stop"),
                    ("assistant_message", "content_block_stop"),
                    ("assistant_delta", "content_block_delta"),
                    ("tool_call", "tool_use"),
                    ("tool_result", "tool_result"),
                ],
            ),
            (
                "gemini",
                &[
                    ("run_started", "generate_content_start"),
                    ("run_completed", "generate_content_end"),
                    ("assistant_message", "text"),
                    ("assistant_delta", "text_delta"),
                    ("tool_call", "function_call"),
                    ("tool_result", "function_response"),
                ],
            ),
            (
                "codex",
                &[
                    ("run_started", "response.created"),
                    ("run_completed", "response.completed"),
                    ("assistant_message", "response.output_text.done"),
                    ("assistant_delta", "response.output_text.delta"),
                    ("tool_call", "function_call"),
                    ("tool_result", "function_call_output"),
                ],
            ),
            (
                "kimi",
                &[
                    ("run_started", "run_started"),
                    ("run_completed", "run_completed"),
                    ("assistant_message", "assistant_message"),
                    ("assistant_delta", "assistant_delta"),
                    ("tool_call", "tool_call"),
                    ("tool_result", "tool_result"),
                ],
            ),
            (
                "mock",
                &[
                    ("run_started", "run_started"),
                    ("run_completed", "run_completed"),
                    ("assistant_message", "assistant_message"),
                    ("assistant_delta", "assistant_delta"),
                    ("tool_call", "tool_call"),
                    ("tool_result", "tool_result"),
                ],
            ),
        ];

        for (from_name, from_events) in event_dialects {
            for (to_name, to_events) in event_dialects {
                if from_name == to_name {
                    continue;
                }
                let key = ((*from_name).to_string(), (*to_name).to_string());
                if self.event_mappings.contains_key(&key) {
                    continue;
                }
                let mappings: Vec<(&str, &str)> = from_events
                    .iter()
                    .filter_map(|(abp_name, from_event)| {
                        to_events
                            .iter()
                            .find(|(abp_n, _)| abp_n == abp_name)
                            .map(|(_, to_event)| (*from_event, *to_event))
                    })
                    .filter(|(f, t)| f != t)
                    .collect();
                self.register_event_mapping(from_name, to_name, &mappings);
            }
        }
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

/// Maps a [`Dialect`] enum variant to its string-based registry name.
fn dialect_to_str(d: Dialect) -> &'static str {
    match d {
        Dialect::Abp => "abp",
        Dialect::Claude => "anthropic",
        Dialect::Codex => "codex",
        Dialect::Gemini => "gemini",
        Dialect::Kimi => "kimi",
        Dialect::Mock => "mock",
        Dialect::OpenAi => "openai",
    }
}

/// Returns `true` if the model name is native to the given dialect.
fn model_belongs_to(model: &str, dialect: Dialect) -> bool {
    match dialect {
        Dialect::OpenAi => {
            model.starts_with("gpt-") || model.starts_with("o1") || model.starts_with("o3")
        }
        Dialect::Claude => model.starts_with("claude-"),
        Dialect::Gemini => model.starts_with("gemini-"),
        Dialect::Codex => model.starts_with("codex-"),
        Dialect::Kimi => model.starts_with("moonshot-"),
        Dialect::Mock | Dialect::Abp => true,
    }
}

/// Looks up a model in the equivalence table and returns the equivalent
/// model name for the target dialect, if one exists.
fn find_equivalent_model(model: &str, target: Dialect) -> Option<&'static str> {
    // Each row: (openai, claude, gemini, codex, kimi)
    // Empty string means no known equivalent in that dialect.
    const GROUPS: &[(&str, &str, &str, &str, &str)] = &[
        (
            "gpt-4o",
            "claude-sonnet-4-20250514",
            "gemini-2.5-flash",
            "codex-mini-latest",
            "moonshot-v1-8k",
        ),
        (
            "gpt-4-turbo",
            "claude-3-5-haiku-20241022",
            "gemini-2.0-flash",
            "",
            "moonshot-v1-32k",
        ),
        (
            "gpt-4o-mini",
            "claude-haiku-4-20250514",
            "gemini-2.0-flash-lite",
            "",
            "",
        ),
    ];

    for &(openai, claude, gemini, codex, kimi) in GROUPS {
        let all = [openai, claude, gemini, codex, kimi];
        if all.contains(&model) {
            let target_model = match target {
                Dialect::OpenAi => openai,
                Dialect::Claude => claude,
                Dialect::Gemini => gemini,
                Dialect::Codex => codex,
                Dialect::Kimi => kimi,
                Dialect::Abp | Dialect::Mock => "",
            };
            if !target_model.is_empty() {
                return Some(target_model);
            }
        }
    }
    None
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

fn wo_to_openai(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "gpt-4o"),
        "messages": [{
            "role": "user",
            "content": build_user_content(wo),
        }],
        "max_tokens": 4096,
    })
}

fn wo_to_mock(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "mock-default"),
        "messages": [{
            "role": "user",
            "content": build_user_content(wo),
        }],
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
            Dialect::Mock => wo_to_mock(wo),
            Dialect::OpenAi => wo_to_openai(wo),
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
