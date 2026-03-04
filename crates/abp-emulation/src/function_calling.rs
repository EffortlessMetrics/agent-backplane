// SPDX-License-Identifier: MIT OR Apache-2.0
//! Function-calling emulation for backends without native support.
//!
//! Some SDKs use a "function calling" interface distinct from the newer "tool
//! use" paradigm. [`FunctionCallingEmulator`] bridges the two by converting
//! between function definitions and tool definitions, and by emulating
//! function calls through system-prompt injection when neither is natively
//! available.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use serde::{Deserialize, Serialize};

/// A function definition in the legacy function-calling format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for parameters.
    pub parameters: serde_json::Value,
}

/// A parsed function call from model output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionCall {
    /// Function name.
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: serde_json::Value,
}

/// Result of function-calling emulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionCallingResult {
    /// Number of functions injected as prompt instructions.
    pub functions_injected: usize,
    /// Whether the conversation was modified.
    pub modified: bool,
}

const FN_CALL_START: &str = "<function_call>";
const FN_CALL_END: &str = "</function_call>";

/// Emulates function calling for backends that lack native support.
///
/// Provides bidirectional conversion between the legacy function-calling
/// format and the modern tool-use format, plus prompt-based emulation
/// when neither is available.
#[derive(Debug, Clone)]
pub struct FunctionCallingEmulator {
    functions: Vec<FunctionDef>,
}

impl FunctionCallingEmulator {
    /// Create with the given function definitions.
    #[must_use]
    pub fn new(functions: Vec<FunctionDef>) -> Self {
        Self { functions }
    }

    /// Create with no functions.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            functions: Vec::new(),
        }
    }

    /// The configured function definitions.
    #[must_use]
    pub fn functions(&self) -> &[FunctionDef] {
        &self.functions
    }

    // ── Format conversion ──────────────────────────────────────────────

    /// Convert a [`FunctionDef`] to an [`IrToolDefinition`].
    #[must_use]
    pub fn function_to_tool(func: &FunctionDef) -> IrToolDefinition {
        IrToolDefinition {
            name: func.name.clone(),
            description: func.description.clone(),
            parameters: func.parameters.clone(),
        }
    }

    /// Convert an [`IrToolDefinition`] to a [`FunctionDef`].
    #[must_use]
    pub fn tool_to_function(tool: &IrToolDefinition) -> FunctionDef {
        FunctionDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
        }
    }

    /// Convert all configured functions to tool definitions.
    #[must_use]
    pub fn as_tool_definitions(&self) -> Vec<IrToolDefinition> {
        self.functions.iter().map(Self::function_to_tool).collect()
    }

    /// Create from tool definitions.
    #[must_use]
    pub fn from_tools(tools: &[IrToolDefinition]) -> Self {
        Self {
            functions: tools.iter().map(Self::tool_to_function).collect(),
        }
    }

    /// Convert a [`FunctionCall`] to an [`IrContentBlock::ToolUse`].
    #[must_use]
    pub fn call_to_tool_use(call: &FunctionCall, id: &str) -> IrContentBlock {
        IrContentBlock::ToolUse {
            id: id.to_string(),
            name: call.name.clone(),
            input: call.arguments.clone(),
        }
    }

    // ── Prompt injection ───────────────────────────────────────────────

    /// Build a system prompt describing available functions.
    #[must_use]
    pub fn functions_to_prompt(functions: &[FunctionDef]) -> String {
        if functions.is_empty() {
            return String::new();
        }

        let mut prompt = String::from("You have access to the following functions:\n\n");

        for func in functions {
            prompt.push_str(&format!("### {}\n", func.name));
            prompt.push_str(&format!("Description: {}\n", func.description));
            if !func.parameters.is_null() {
                if let Ok(pretty) = serde_json::to_string_pretty(&func.parameters) {
                    prompt.push_str(&format!("Parameters: {pretty}\n"));
                }
            }
            prompt.push('\n');
        }

        prompt.push_str(concat!(
            "To call a function, respond with a <function_call> block:\n",
            "<function_call>\n",
            "{\"name\": \"function_name\", \"arguments\": {\"arg1\": \"value1\"}}\n",
            "</function_call>\n\n",
            "You may call multiple functions by including multiple <function_call> blocks.\n",
            "Only call functions listed above.",
        ));

        prompt
    }

    /// Inject function definitions into a conversation's system prompt.
    pub fn inject(&self, conv: &mut IrConversation) -> FunctionCallingResult {
        if self.functions.is_empty() {
            return FunctionCallingResult {
                functions_injected: 0,
                modified: false,
            };
        }

        let prompt = Self::functions_to_prompt(&self.functions);
        if let Some(sys) = conv.messages.iter_mut().find(|m| m.role == IrRole::System) {
            sys.content.push(IrContentBlock::Text {
                text: format!("\n{prompt}"),
            });
        } else {
            conv.messages
                .insert(0, IrMessage::text(IrRole::System, &prompt));
        }

        FunctionCallingResult {
            functions_injected: self.functions.len(),
            modified: true,
        }
    }

    // ── Response parsing ───────────────────────────────────────────────

    /// Parse function calls from a text response containing `<function_call>` blocks.
    #[must_use]
    pub fn parse_calls(text: &str) -> Vec<Result<FunctionCall, String>> {
        let mut results = Vec::new();
        let mut search_from = 0;

        while let Some(start) = text[search_from..].find(FN_CALL_START) {
            let abs_start = search_from + start + FN_CALL_START.len();
            if let Some(end) = text[abs_start..].find(FN_CALL_END) {
                let abs_end = abs_start + end;
                let json_str = text[abs_start..abs_end].trim();

                match serde_json::from_str::<serde_json::Value>(json_str) {
                    Ok(val) => {
                        let name = val
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = val
                            .get("arguments")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        if name.is_empty() {
                            results.push(Err("function_call missing 'name' field".to_string()));
                        } else {
                            results.push(Ok(FunctionCall { name, arguments }));
                        }
                    }
                    Err(e) => {
                        results.push(Err(format!("invalid JSON in function_call: {e}")));
                    }
                }
                search_from = abs_end + FN_CALL_END.len();
            } else {
                results.push(Err("unclosed <function_call> tag".to_string()));
                break;
            }
        }
        results
    }

    /// Parse function calls and validate against registered functions.
    #[must_use]
    pub fn parse_and_validate(&self, text: &str) -> FunctionParseResult {
        let raw = Self::parse_calls(text);
        let mut valid = Vec::new();
        let mut unknown = Vec::new();
        let mut errors = Vec::new();

        for result in raw {
            match result {
                Ok(call) => {
                    if self.functions.iter().any(|f| f.name == call.name) {
                        valid.push(call);
                    } else {
                        unknown.push(call.name.clone());
                    }
                }
                Err(e) => errors.push(e),
            }
        }

        let text_outside = Self::extract_text_outside_calls(text);

        FunctionParseResult {
            valid_calls: valid,
            unknown_functions: unknown,
            parse_errors: errors,
            text_outside,
        }
    }

    /// Extract text outside `<function_call>` blocks.
    #[must_use]
    pub fn extract_text_outside_calls(text: &str) -> String {
        let mut result = String::new();
        let mut search_from = 0;

        while let Some(start) = text[search_from..].find(FN_CALL_START) {
            let abs_start = search_from + start;
            result.push_str(&text[search_from..abs_start]);
            if let Some(end) = text[abs_start..].find(FN_CALL_END) {
                search_from = abs_start + end + FN_CALL_END.len();
            } else {
                break;
            }
        }
        result.push_str(&text[search_from..]);
        result.trim().to_string()
    }

    /// Format a function result for re-injection into the conversation.
    #[must_use]
    pub fn format_result(name: &str, result: &str, is_error: bool) -> String {
        if is_error {
            format!("Function '{name}' returned an error:\n{result}")
        } else {
            format!("Function '{name}' returned:\n{result}")
        }
    }
}

/// Result of parsing and validating function calls from text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionParseResult {
    /// Successfully parsed and validated function calls.
    pub valid_calls: Vec<FunctionCall>,
    /// Function names parsed but not in the registered set.
    pub unknown_functions: Vec<String>,
    /// Raw parse errors.
    pub parse_errors: Vec<String>,
    /// Text outside of function call blocks.
    pub text_outside: String,
}

impl FunctionParseResult {
    /// Returns `true` if nothing was parsed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.valid_calls.is_empty()
            && self.unknown_functions.is_empty()
            && self.parse_errors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_functions() -> Vec<FunctionDef> {
        vec![
            FunctionDef {
                name: "get_weather".into(),
                description: "Get weather for a location".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    }
                }),
            },
            FunctionDef {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }),
            },
        ]
    }

    #[test]
    fn function_to_tool_conversion() {
        let func = &sample_functions()[0];
        let tool = FunctionCallingEmulator::function_to_tool(func);
        assert_eq!(tool.name, func.name);
        assert_eq!(tool.description, func.description);
        assert_eq!(tool.parameters, func.parameters);
    }

    #[test]
    fn tool_to_function_conversion() {
        let tool = IrToolDefinition {
            name: "test".into(),
            description: "A test tool".into(),
            parameters: serde_json::json!({}),
        };
        let func = FunctionCallingEmulator::tool_to_function(&tool);
        assert_eq!(func.name, tool.name);
        assert_eq!(func.description, tool.description);
    }

    #[test]
    fn roundtrip_function_tool_conversion() {
        let original = &sample_functions()[0];
        let tool = FunctionCallingEmulator::function_to_tool(original);
        let back = FunctionCallingEmulator::tool_to_function(&tool);
        assert_eq!(back, *original);
    }

    #[test]
    fn from_tools_creates_emulator() {
        let tools = vec![IrToolDefinition {
            name: "t1".into(),
            description: "d1".into(),
            parameters: serde_json::Value::Null,
        }];
        let emu = FunctionCallingEmulator::from_tools(&tools);
        assert_eq!(emu.functions().len(), 1);
        assert_eq!(emu.functions()[0].name, "t1");
    }

    #[test]
    fn inject_empty_does_nothing() {
        let emu = FunctionCallingEmulator::empty();
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let result = emu.inject(&mut conv);
        assert_eq!(result.functions_injected, 0);
        assert!(!result.modified);
    }

    #[test]
    fn inject_adds_system_prompt() {
        let emu = FunctionCallingEmulator::new(sample_functions());
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let result = emu.inject(&mut conv);
        assert_eq!(result.functions_injected, 2);
        assert!(result.modified);
        let sys = conv.system_message().unwrap().text_content();
        assert!(sys.contains("get_weather"));
        assert!(sys.contains("search"));
        assert!(sys.contains("<function_call>"));
    }

    #[test]
    fn inject_appends_to_existing_system() {
        let emu = FunctionCallingEmulator::new(sample_functions());
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "hi"));
        emu.inject(&mut conv);
        let sys = conv.system_message().unwrap().text_content();
        assert!(sys.contains("You are helpful."));
        assert!(sys.contains("get_weather"));
    }

    #[test]
    fn parse_single_call() {
        let text = r#"<function_call>
{"name": "get_weather", "arguments": {"location": "NYC"}}
</function_call>"#;
        let results = FunctionCallingEmulator::parse_calls(text);
        assert_eq!(results.len(), 1);
        let call = results[0].as_ref().unwrap();
        assert_eq!(call.name, "get_weather");
    }

    #[test]
    fn parse_multiple_calls() {
        let text = r#"<function_call>
{"name": "get_weather", "arguments": {"location": "NYC"}}
</function_call>
<function_call>
{"name": "search", "arguments": {"query": "rust"}}
</function_call>"#;
        let results = FunctionCallingEmulator::parse_calls(text);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn parse_invalid_json() {
        let text = "<function_call>\nnot json\n</function_call>";
        let results = FunctionCallingEmulator::parse_calls(text);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn parse_missing_name() {
        let text = r#"<function_call>
{"arguments": {"x": 1}}
</function_call>"#;
        let results = FunctionCallingEmulator::parse_calls(text);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn parse_unclosed_tag() {
        let text = "<function_call>\n{\"name\": \"x\", \"arguments\": {}}";
        let results = FunctionCallingEmulator::parse_calls(text);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
        assert!(results[0].as_ref().unwrap_err().contains("unclosed"));
    }

    #[test]
    fn parse_and_validate_filters_unknown() {
        let emu = FunctionCallingEmulator::new(sample_functions());
        let text = r#"<function_call>
{"name": "get_weather", "arguments": {"location": "NYC"}}
</function_call>
<function_call>
{"name": "unknown_func", "arguments": {}}
</function_call>"#;
        let result = emu.parse_and_validate(text);
        assert_eq!(result.valid_calls.len(), 1);
        assert_eq!(result.unknown_functions, vec!["unknown_func"]);
    }

    #[test]
    fn extract_text_outside_calls() {
        let text =
            "Hello <function_call>\n{\"name\": \"x\", \"arguments\": {}}\n</function_call> world";
        let outside = FunctionCallingEmulator::extract_text_outside_calls(text);
        assert!(outside.contains("Hello"));
        assert!(outside.contains("world"));
        assert!(!outside.contains("function_call"));
    }

    #[test]
    fn call_to_tool_use_conversion() {
        let call = FunctionCall {
            name: "test".into(),
            arguments: serde_json::json!({"a": 1}),
        };
        let block = FunctionCallingEmulator::call_to_tool_use(&call, "id-1");
        match block {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "id-1");
                assert_eq!(name, "test");
                assert_eq!(input, serde_json::json!({"a": 1}));
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn format_result_success() {
        let s = FunctionCallingEmulator::format_result("f1", "42", false);
        assert!(s.contains("f1"));
        assert!(s.contains("42"));
        assert!(!s.contains("error"));
    }

    #[test]
    fn format_result_error() {
        let s = FunctionCallingEmulator::format_result("f1", "fail", true);
        assert!(s.contains("error"));
    }

    #[test]
    fn serde_roundtrip_function_def() {
        let func = &sample_functions()[0];
        let json = serde_json::to_string(func).unwrap();
        let decoded: FunctionDef = serde_json::from_str(&json).unwrap();
        assert_eq!(*func, decoded);
    }

    #[test]
    fn serde_roundtrip_function_call() {
        let call = FunctionCall {
            name: "test".into(),
            arguments: serde_json::json!({"key": "value"}),
        };
        let json = serde_json::to_string(&call).unwrap();
        let decoded: FunctionCall = serde_json::from_str(&json).unwrap();
        assert_eq!(call, decoded);
    }

    #[test]
    fn as_tool_definitions_converts_all() {
        let emu = FunctionCallingEmulator::new(sample_functions());
        let tools = emu.as_tool_definitions();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "get_weather");
        assert_eq!(tools[1].name, "search");
    }

    #[test]
    fn parse_no_calls_returns_empty() {
        let text = "Just regular text with no function calls.";
        let results = FunctionCallingEmulator::parse_calls(text);
        assert!(results.is_empty());
    }

    #[test]
    fn function_parse_result_is_empty() {
        let r = FunctionParseResult {
            valid_calls: vec![],
            unknown_functions: vec![],
            parse_errors: vec![],
            text_outside: String::new(),
        };
        assert!(r.is_empty());
    }
}
