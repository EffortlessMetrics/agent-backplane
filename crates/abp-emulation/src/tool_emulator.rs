// SPDX-License-Identifier: MIT OR Apache-2.0
//! High-level tool emulation for backends lacking native tool-use support.
//!
//! [`ToolEmulator`] wraps the low-level [`ToolUseEmulation`] strategy with
//! additional capabilities: tool filtering, schema validation of parsed calls,
//! and batch processing.

use crate::strategies::{ParsedToolCall, ToolUseEmulation};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use serde::{Deserialize, Serialize};

/// Result of a tool emulation pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEmulationResult {
    /// Number of tool definitions injected into the system prompt.
    pub tools_injected: usize,
    /// Whether the system prompt was modified.
    pub prompt_modified: bool,
}

/// High-level tool emulator that orchestrates prompt injection, response
/// parsing, and schema validation for backends without native tool support.
#[derive(Debug, Clone)]
pub struct ToolEmulator {
    tools: Vec<IrToolDefinition>,
}

impl ToolEmulator {
    /// Create a new emulator with the given tool definitions.
    #[must_use]
    pub fn new(tools: Vec<IrToolDefinition>) -> Self {
        Self { tools }
    }

    /// Create an emulator with no tools.
    #[must_use]
    pub fn empty() -> Self {
        Self { tools: Vec::new() }
    }

    /// The configured tool definitions.
    #[must_use]
    pub fn tools(&self) -> &[IrToolDefinition] {
        &self.tools
    }

    /// Filter tools to only those whose names appear in `allowed`.
    #[must_use]
    pub fn filter_by_names(&self, allowed: &[&str]) -> Vec<IrToolDefinition> {
        self.tools
            .iter()
            .filter(|t| allowed.contains(&t.name.as_str()))
            .cloned()
            .collect()
    }

    /// Inject tool definitions into a conversation's system prompt.
    ///
    /// Returns metadata about what was injected.
    pub fn inject(&self, conv: &mut IrConversation) -> ToolEmulationResult {
        if self.tools.is_empty() {
            return ToolEmulationResult {
                tools_injected: 0,
                prompt_modified: false,
            };
        }
        ToolUseEmulation::inject_tools(conv, &self.tools);
        ToolEmulationResult {
            tools_injected: self.tools.len(),
            prompt_modified: true,
        }
    }

    /// Parse tool calls from assistant text and convert to [`IrContentBlock::ToolUse`] blocks.
    ///
    /// Returns `(blocks, errors)` where errors are parsing failures.
    #[must_use]
    pub fn parse_response(&self, text: &str) -> (Vec<IrContentBlock>, Vec<String>) {
        let raw = ToolUseEmulation::parse_tool_calls(text);
        let mut blocks = Vec::new();
        let mut errors = Vec::new();

        for (i, result) in raw.into_iter().enumerate() {
            match result {
                Ok(call) => {
                    let id = format!("emulated-{i}");
                    blocks.push(ToolUseEmulation::to_tool_use_block(&call, &id));
                }
                Err(e) => errors.push(e),
            }
        }
        (blocks, errors)
    }

    /// Validate a parsed tool call against the registered tool definitions.
    ///
    /// Returns `Ok(())` if the tool name exists, or an error message.
    pub fn validate_call(&self, call: &ParsedToolCall) -> Result<(), String> {
        if self.tools.iter().any(|t| t.name == call.name) {
            Ok(())
        } else {
            Err(format!(
                "Tool '{}' not found in registered definitions",
                call.name
            ))
        }
    }

    /// Parse, validate, and convert tool calls from response text.
    ///
    /// Only tool calls whose names match registered tools are included.
    #[must_use]
    pub fn parse_and_validate(&self, text: &str) -> ToolParseResult {
        let raw = ToolUseEmulation::parse_tool_calls(text);
        let mut valid = Vec::new();
        let mut invalid = Vec::new();
        let mut parse_errors = Vec::new();

        for result in raw {
            match result {
                Ok(call) => {
                    if self.validate_call(&call).is_ok() {
                        valid.push(call);
                    } else {
                        invalid.push(call.name.clone());
                    }
                }
                Err(e) => parse_errors.push(e),
            }
        }

        let text_outside = ToolUseEmulation::extract_text_outside_tool_calls(text);

        ToolParseResult {
            valid_calls: valid,
            unknown_tools: invalid,
            parse_errors,
            text_outside,
        }
    }

    /// Format a tool result for re-injection into the conversation.
    pub fn inject_result(
        conv: &mut IrConversation,
        tool_name: &str,
        result_text: &str,
        is_error: bool,
    ) {
        let formatted = ToolUseEmulation::format_tool_result(tool_name, result_text, is_error);
        conv.messages
            .push(IrMessage::text(IrRole::User, &formatted));
    }
}

/// Result of parsing and validating tool calls from text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolParseResult {
    /// Successfully parsed and validated tool calls.
    pub valid_calls: Vec<ParsedToolCall>,
    /// Tool names that were parsed but not in the registered set.
    pub unknown_tools: Vec<String>,
    /// Raw parse errors.
    pub parse_errors: Vec<String>,
    /// Text content outside of tool call blocks.
    pub text_outside: String,
}

impl ToolParseResult {
    /// Returns `true` if there are no valid calls, unknown tools, or errors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.valid_calls.is_empty() && self.unknown_tools.is_empty() && self.parse_errors.is_empty()
    }

    /// Returns `true` if there were any errors (parse or validation).
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.unknown_tools.is_empty() || !self.parse_errors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tools() -> Vec<IrToolDefinition> {
        vec![
            IrToolDefinition {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
            IrToolDefinition {
                name: "write_file".into(),
                description: "Write a file".into(),
                parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
            },
        ]
    }

    #[test]
    fn empty_emulator_injects_nothing() {
        let emu = ToolEmulator::empty();
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let result = emu.inject(&mut conv);
        assert_eq!(result.tools_injected, 0);
        assert!(!result.prompt_modified);
    }

    #[test]
    fn inject_adds_system_prompt() {
        let emu = ToolEmulator::new(sample_tools());
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let result = emu.inject(&mut conv);
        assert_eq!(result.tools_injected, 2);
        assert!(result.prompt_modified);
        assert!(conv.system_message().is_some());
    }

    #[test]
    fn filter_by_names_returns_subset() {
        let emu = ToolEmulator::new(sample_tools());
        let filtered = emu.filter_by_names(&["read_file"]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "read_file");
    }

    #[test]
    fn filter_by_names_empty_allowed() {
        let emu = ToolEmulator::new(sample_tools());
        let filtered = emu.filter_by_names(&[]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn validate_call_known_tool() {
        let emu = ToolEmulator::new(sample_tools());
        let call = ParsedToolCall {
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "/tmp/test"}),
        };
        assert!(emu.validate_call(&call).is_ok());
    }

    #[test]
    fn validate_call_unknown_tool() {
        let emu = ToolEmulator::new(sample_tools());
        let call = ParsedToolCall {
            name: "delete_file".into(),
            arguments: serde_json::Value::Null,
        };
        assert!(emu.validate_call(&call).is_err());
    }

    #[test]
    fn parse_and_validate_good_call() {
        let emu = ToolEmulator::new(sample_tools());
        let text = r#"<tool_call>
{"name": "read_file", "arguments": {"path": "/tmp/test"}}
</tool_call>"#;
        let result = emu.parse_and_validate(text);
        assert_eq!(result.valid_calls.len(), 1);
        assert!(result.unknown_tools.is_empty());
        assert!(result.parse_errors.is_empty());
    }

    #[test]
    fn parse_and_validate_unknown_tool_name() {
        let emu = ToolEmulator::new(sample_tools());
        let text = r#"<tool_call>
{"name": "nope", "arguments": {}}
</tool_call>"#;
        let result = emu.parse_and_validate(text);
        assert!(result.valid_calls.is_empty());
        assert_eq!(result.unknown_tools, vec!["nope"]);
    }

    #[test]
    fn parse_and_validate_preserves_text_outside() {
        let emu = ToolEmulator::new(sample_tools());
        let text = r#"Hello world <tool_call>
{"name": "read_file", "arguments": {"path": "x"}}
</tool_call> more text"#;
        let result = emu.parse_and_validate(text);
        assert!(result.text_outside.contains("Hello world"));
        assert!(result.text_outside.contains("more text"));
    }

    #[test]
    fn parse_response_converts_to_blocks() {
        let emu = ToolEmulator::new(sample_tools());
        let text = r#"<tool_call>
{"name": "read_file", "arguments": {"path": "test.txt"}}
</tool_call>"#;
        let (blocks, errors) = emu.parse_response(text);
        assert_eq!(blocks.len(), 1);
        assert!(errors.is_empty());
        assert!(matches!(blocks[0], IrContentBlock::ToolUse { .. }));
    }

    #[test]
    fn inject_result_adds_user_message() {
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        ToolEmulator::inject_result(&mut conv, "read_file", "file contents", false);
        assert_eq!(conv.messages.len(), 2);
        let text = conv.messages[1].text_content();
        assert!(text.contains("read_file"));
        assert!(text.contains("file contents"));
    }

    #[test]
    fn inject_result_error_format() {
        let mut conv = IrConversation::new();
        ToolEmulator::inject_result(&mut conv, "bad_tool", "not found", true);
        let text = conv.messages[0].text_content();
        assert!(text.contains("error"));
    }

    #[test]
    fn tool_parse_result_is_empty() {
        let r = ToolParseResult {
            valid_calls: vec![],
            unknown_tools: vec![],
            parse_errors: vec![],
            text_outside: String::new(),
        };
        assert!(r.is_empty());
        assert!(!r.has_errors());
    }

    #[test]
    fn tool_parse_result_has_errors_with_unknown() {
        let r = ToolParseResult {
            valid_calls: vec![],
            unknown_tools: vec!["bad".into()],
            parse_errors: vec![],
            text_outside: String::new(),
        };
        assert!(r.has_errors());
    }
}
