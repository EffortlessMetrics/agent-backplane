// SPDX-License-Identifier: MIT OR Apache-2.0
//! Projection matrix for translating between vendor dialects.
//!
//! In v0.1, the matrix supports:
//! - **Identity translations**: same dialect in and out (pass-through).
//! - **ABP-to-vendor translations**: convert an ABP [`WorkOrder`] into the
//!   vendor-specific request JSON for each supported dialect.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
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

// ── IR-based cross-dialect message translation ──────────────────────────

/// Report produced by [`map_via_ir`] describing fidelity and losses
/// incurred during cross-dialect message translation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationReport {
    /// Source dialect for the translation.
    pub source_dialect: Dialect,
    /// Target dialect for the translation.
    pub target_dialect: Dialect,
    /// Number of messages that were mapped.
    pub messages_mapped: usize,
    /// Descriptions of information lost during translation.
    pub losses: Vec<String>,
    /// Overall fidelity assessment of the translation.
    pub fidelity: TranslationFidelity,
}

/// Extended model equivalence table mapping models across vendor dialects.
///
/// Each row contains `(openai, claude, gemini, codex, kimi)` equivalents.
/// An empty string means no known equivalent in that dialect.
pub const MODEL_EQUIVALENCE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
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
    (
        "gpt-4.1",
        "claude-sonnet-4-latest",
        "gemini-2.5-pro",
        "",
        "",
    ),
    ("o1", "claude-opus-4-20250514", "gemini-1.5-pro", "", ""),
    (
        "o3-mini",
        "claude-3-5-haiku-latest",
        "gemini-1.5-flash",
        "",
        "",
    ),
];

/// Translate a model name from one dialect to another using the
/// equivalence table.
///
/// Returns `None` if the model is not found in the table or the target
/// dialect has no known equivalent.
#[must_use]
pub fn translate_model_name(model: &str, target: Dialect) -> Option<String> {
    if target == Dialect::Abp || target == Dialect::Mock {
        return Some(model.to_string());
    }
    for &(openai, claude, gemini, codex, kimi) in MODEL_EQUIVALENCE_TABLE {
        let all = [openai, claude, gemini, codex, kimi];
        if all.contains(&model) {
            let target_model = match target {
                Dialect::OpenAi => openai,
                Dialect::Claude => claude,
                Dialect::Gemini => gemini,
                Dialect::Codex => codex,
                Dialect::Kimi => kimi,
                Dialect::Abp | Dialect::Mock => unreachable!(),
            };
            if !target_model.is_empty() {
                return Some(target_model.to_string());
            }
            return None;
        }
    }
    None
}

/// Detect the likely source dialect from a JSON messages array.
///
/// Uses structural heuristics:
/// - **Gemini**: messages contain a `parts` array.
/// - **OpenAI**: messages contain `tool_calls`, `tool_call_id`, or a
///   `system`/`tool` role.
/// - **Claude**: messages have a string `content` field with only
///   `user`/`assistant` roles.
///
/// Returns `None` if the array is empty or detection is ambiguous.
#[must_use]
pub fn detect_dialect(messages: &serde_json::Value) -> Option<Dialect> {
    let arr = messages.as_array()?;
    let first = arr.first()?;

    if first.get("parts").is_some() {
        return Some(Dialect::Gemini);
    }

    for msg in arr {
        if msg.get("tool_calls").is_some() || msg.get("tool_call_id").is_some() {
            return Some(Dialect::OpenAi);
        }
        let role = msg.get("role").and_then(|r| r.as_str());
        if role == Some("system") || role == Some("tool") {
            return Some(Dialect::OpenAi);
        }
    }

    Some(Dialect::Claude)
}

/// Translate messages from one dialect to another via the ABP IR.
///
/// Lowers source dialect messages into an [`IrConversation`], then raises
/// them into the target dialect format.  Returns the mapped messages as a
/// JSON array along with a [`TranslationReport`].
///
/// # Errors
///
/// Returns an error if the input is not a JSON array or cannot be parsed
/// as the given source dialect format.
pub fn map_via_ir(
    source: Dialect,
    target: Dialect,
    messages: &serde_json::Value,
) -> Result<(serde_json::Value, TranslationReport)> {
    if source == target {
        let count = messages.as_array().map_or(0, Vec::len);
        return Ok((
            messages.clone(),
            TranslationReport {
                source_dialect: source,
                target_dialect: target,
                messages_mapped: count,
                losses: vec![],
                fidelity: TranslationFidelity::Lossless,
            },
        ));
    }

    let arr = messages
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("messages must be a JSON array"))?;

    let (conv, mut losses) = ir_lower(source, arr)?;
    let (output, raise_losses) = ir_raise(target, &conv)?;
    losses.extend(raise_losses);

    let fidelity = if losses.is_empty() {
        TranslationFidelity::LossySupported
    } else {
        TranslationFidelity::Degraded
    };

    Ok((
        output,
        TranslationReport {
            source_dialect: source,
            target_dialect: target,
            messages_mapped: conv.len(),
            losses,
            fidelity,
        },
    ))
}

// ── IR lowering (dialect JSON → IrConversation) ─────────────────────────

fn ir_lower(
    dialect: Dialect,
    messages: &[serde_json::Value],
) -> Result<(IrConversation, Vec<String>)> {
    match dialect {
        Dialect::OpenAi | Dialect::Codex | Dialect::Kimi | Dialect::Mock | Dialect::Abp => {
            Ok(ir_lower_openai(messages))
        }
        Dialect::Claude => Ok(ir_lower_claude(messages)),
        Dialect::Gemini => Ok(ir_lower_gemini(messages)),
    }
}

fn ir_lower_openai(messages: &[serde_json::Value]) -> (IrConversation, Vec<String>) {
    let mut ir_msgs = Vec::new();
    let losses = Vec::new();

    for msg in messages {
        let role = match msg.get("role").and_then(|r| r.as_str()) {
            Some("system") => IrRole::System,
            Some("assistant") => IrRole::Assistant,
            Some("tool") => IrRole::Tool,
            _ => IrRole::User,
        };

        let mut blocks = Vec::new();
        if let Some(text) = msg.get("content").and_then(|c| c.as_str())
            && !text.is_empty()
        {
            blocks.push(IrContentBlock::Text {
                text: text.to_string(),
            });
        }

        if let Some(tool_calls) = msg.get("tool_calls").and_then(|tc| tc.as_array()) {
            for tc in tool_calls {
                let id = tc
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let args_str = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");
                let input: serde_json::Value = serde_json::from_str(args_str)
                    .unwrap_or(serde_json::Value::String(args_str.to_string()));
                blocks.push(IrContentBlock::ToolUse { id, name, input });
            }
        }

        if role == IrRole::Tool
            && let Some(tcid) = msg.get("tool_call_id").and_then(|v| v.as_str())
        {
            let content_blocks = msg
                .get("content")
                .and_then(|c| c.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    vec![IrContentBlock::Text {
                        text: s.to_string(),
                    }]
                })
                .unwrap_or_default();
            blocks = vec![IrContentBlock::ToolResult {
                tool_use_id: tcid.to_string(),
                content: content_blocks,
                is_error: false,
            }];
        }

        ir_msgs.push(IrMessage::new(role, blocks));
    }

    (IrConversation::from_messages(ir_msgs), losses)
}

fn ir_lower_claude(messages: &[serde_json::Value]) -> (IrConversation, Vec<String>) {
    let mut ir_msgs = Vec::new();
    let mut losses = Vec::new();

    for msg in messages {
        let role = match msg.get("role").and_then(|r| r.as_str()) {
            Some("assistant") => IrRole::Assistant,
            _ => IrRole::User,
        };

        let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");

        if let Ok(blocks) = serde_json::from_str::<Vec<serde_json::Value>>(content)
            && !blocks.is_empty()
            && blocks[0].get("type").is_some()
        {
            let ir_blocks = claude_blocks_to_ir(&blocks, &mut losses);
            ir_msgs.push(IrMessage::new(role, ir_blocks));
            continue;
        }

        ir_msgs.push(IrMessage::text(role, content));
    }

    (IrConversation::from_messages(ir_msgs), losses)
}

fn claude_blocks_to_ir(
    blocks: &[serde_json::Value],
    losses: &mut Vec<String>,
) -> Vec<IrContentBlock> {
    let mut ir_blocks = Vec::new();
    for block in blocks {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    ir_blocks.push(IrContentBlock::Text {
                        text: text.to_string(),
                    });
                }
            }
            Some("tool_use") => {
                let id = block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = block.get("input").cloned().unwrap_or(json!({}));
                ir_blocks.push(IrContentBlock::ToolUse { id, name, input });
            }
            Some("tool_result") => {
                let tool_use_id = block
                    .get("tool_use_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let inner = block
                    .get("content")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        vec![IrContentBlock::Text {
                            text: s.to_string(),
                        }]
                    })
                    .unwrap_or_default();
                let is_error = block
                    .get("is_error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                ir_blocks.push(IrContentBlock::ToolResult {
                    tool_use_id,
                    content: inner,
                    is_error,
                });
            }
            Some("thinking") => {
                if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                    ir_blocks.push(IrContentBlock::Thinking {
                        text: text.to_string(),
                    });
                }
                if block.get("signature").is_some() {
                    losses.push("thinking signature dropped".to_string());
                }
            }
            Some("image") => {
                if let Some(source) = block.get("source")
                    && let (Some(mt), Some(d)) = (
                        source.get("media_type").and_then(|v| v.as_str()),
                        source.get("data").and_then(|v| v.as_str()),
                    )
                {
                    ir_blocks.push(IrContentBlock::Image {
                        media_type: mt.to_string(),
                        data: d.to_string(),
                    });
                }
            }
            _ => {}
        }
    }
    ir_blocks
}

fn ir_lower_gemini(messages: &[serde_json::Value]) -> (IrConversation, Vec<String>) {
    let mut ir_msgs = Vec::new();
    let losses = Vec::new();

    for msg in messages {
        let role = match msg.get("role").and_then(|r| r.as_str()) {
            Some("model") => IrRole::Assistant,
            _ => IrRole::User,
        };

        let mut blocks = Vec::new();
        if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
            for part in parts {
                if let Some(text) = part
                    .get("text")
                    .or_else(|| part.get("Text"))
                    .and_then(|t| t.as_str())
                {
                    blocks.push(IrContentBlock::Text {
                        text: text.to_string(),
                    });
                } else if let Some(fc) = part
                    .get("functionCall")
                    .or_else(|| part.get("FunctionCall"))
                {
                    let name = fc
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = fc.get("args").cloned().unwrap_or(json!({}));
                    blocks.push(IrContentBlock::ToolUse {
                        id: format!("gemini_{name}"),
                        name,
                        input: args,
                    });
                } else if let Some(fr) = part
                    .get("functionResponse")
                    .or_else(|| part.get("FunctionResponse"))
                {
                    let name = fr
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let response = fr.get("response").cloned().unwrap_or(json!(null));
                    let text = match &response {
                        serde_json::Value::String(s) => s.clone(),
                        other => serde_json::to_string(other).unwrap_or_default(),
                    };
                    blocks.push(IrContentBlock::ToolResult {
                        tool_use_id: format!("gemini_{name}"),
                        content: vec![IrContentBlock::Text { text }],
                        is_error: false,
                    });
                } else if let Some(data) = part.get("inlineData").or_else(|| part.get("InlineData"))
                {
                    let mime = data
                        .get("mimeType")
                        .or_else(|| data.get("mime_type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let d = data
                        .get("data")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    blocks.push(IrContentBlock::Image {
                        media_type: mime,
                        data: d,
                    });
                }
            }
        }

        ir_msgs.push(IrMessage::new(role, blocks));
    }

    (IrConversation::from_messages(ir_msgs), losses)
}

// ── IR raising (IrConversation → dialect JSON) ──────────────────────────

fn ir_raise(dialect: Dialect, conv: &IrConversation) -> Result<(serde_json::Value, Vec<String>)> {
    match dialect {
        Dialect::OpenAi | Dialect::Codex | Dialect::Kimi | Dialect::Mock | Dialect::Abp => {
            Ok(ir_raise_openai(conv))
        }
        Dialect::Claude => Ok(ir_raise_claude(conv)),
        Dialect::Gemini => Ok(ir_raise_gemini(conv)),
    }
}

fn ir_raise_openai(conv: &IrConversation) -> (serde_json::Value, Vec<String>) {
    let losses = Vec::new();
    let messages: Vec<serde_json::Value> = conv.messages.iter().map(ir_msg_to_openai).collect();
    (serde_json::Value::Array(messages), losses)
}

fn ir_msg_to_openai(msg: &IrMessage) -> serde_json::Value {
    let role = match msg.role {
        IrRole::System => "system",
        IrRole::User => "user",
        IrRole::Assistant => "assistant",
        IrRole::Tool => "tool",
    };

    if msg.role == IrRole::Tool
        && let Some(IrContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        }) = msg.content.first()
    {
        let text: String = content
            .iter()
            .filter_map(|b| match b {
                IrContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        return json!({
            "role": role,
            "content": text,
            "tool_call_id": tool_use_id,
        });
    }

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &msg.content {
        match block {
            IrContentBlock::Text { text } => text_parts.push(text.as_str()),
            IrContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": serde_json::to_string(input).unwrap_or_default(),
                    }
                }));
            }
            IrContentBlock::Thinking { text } => text_parts.push(text.as_str()),
            _ => {}
        }
    }

    let content = if text_parts.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(text_parts.join(""))
    };

    let mut obj = json!({ "role": role, "content": content });
    if !tool_calls.is_empty() {
        obj["tool_calls"] = serde_json::Value::Array(tool_calls);
    }
    obj
}

fn ir_raise_claude(conv: &IrConversation) -> (serde_json::Value, Vec<String>) {
    let mut losses = Vec::new();
    let mut messages = Vec::new();

    for msg in &conv.messages {
        if msg.role == IrRole::System {
            losses.push(
                "system message excluded (Claude uses request-level system field)".to_string(),
            );
            continue;
        }
        messages.push(ir_msg_to_claude(msg));
    }

    (serde_json::Value::Array(messages), losses)
}

fn ir_msg_to_claude(msg: &IrMessage) -> serde_json::Value {
    let role = match msg.role {
        IrRole::Assistant => "assistant",
        _ => "user",
    };

    let has_structured = msg.content.iter().any(|b| {
        matches!(
            b,
            IrContentBlock::ToolUse { .. }
                | IrContentBlock::ToolResult { .. }
                | IrContentBlock::Image { .. }
                | IrContentBlock::Thinking { .. }
        )
    });

    if has_structured {
        let blocks: Vec<serde_json::Value> = msg.content.iter().map(ir_block_to_claude).collect();
        let content = serde_json::to_string(&blocks).unwrap_or_default();
        json!({ "role": role, "content": content })
    } else {
        json!({ "role": role, "content": msg.text_content() })
    }
}

fn ir_block_to_claude(block: &IrContentBlock) -> serde_json::Value {
    match block {
        IrContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        IrContentBlock::ToolUse { id, name, input } => {
            json!({ "type": "tool_use", "id": id, "name": name, "input": input })
        }
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let text: String = content
                .iter()
                .filter_map(|b| match b {
                    IrContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            let mut obj = json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
            });
            if !text.is_empty() {
                obj["content"] = serde_json::Value::String(text);
            }
            if *is_error {
                obj["is_error"] = serde_json::Value::Bool(true);
            }
            obj
        }
        IrContentBlock::Thinking { text } => {
            json!({ "type": "thinking", "thinking": text })
        }
        IrContentBlock::Image { media_type, data } => {
            json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": data,
                }
            })
        }
    }
}

fn ir_raise_gemini(conv: &IrConversation) -> (serde_json::Value, Vec<String>) {
    let mut losses = Vec::new();
    let mut contents = Vec::new();

    for msg in &conv.messages {
        if msg.role == IrRole::System {
            losses.push(
                "system message excluded (Gemini uses request-level system_instruction)"
                    .to_string(),
            );
            continue;
        }
        contents.push(ir_msg_to_gemini(msg));
    }

    (serde_json::Value::Array(contents), losses)
}

fn ir_msg_to_gemini(msg: &IrMessage) -> serde_json::Value {
    let role = match msg.role {
        IrRole::Assistant => "model",
        _ => "user",
    };

    let parts: Vec<serde_json::Value> = msg.content.iter().map(ir_block_to_gemini).collect();
    json!({ "role": role, "parts": parts })
}

fn ir_block_to_gemini(block: &IrContentBlock) -> serde_json::Value {
    match block {
        IrContentBlock::Text { text } => json!({ "text": text }),
        IrContentBlock::ToolUse { name, input, .. } => {
            json!({ "functionCall": { "name": name, "args": input } })
        }
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            let name = tool_use_id
                .strip_prefix("gemini_")
                .unwrap_or(tool_use_id)
                .to_string();
            let text: String = content
                .iter()
                .filter_map(|b| match b {
                    IrContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            json!({ "functionResponse": { "name": name, "response": text } })
        }
        IrContentBlock::Thinking { text } => json!({ "text": text }),
        IrContentBlock::Image { media_type, data } => {
            json!({ "inlineData": { "mimeType": media_type, "data": data } })
        }
    }
}
