// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Extended function calling helpers for Gemini API integration.
//!
//! Provides builders and validation on top of the core types defined in
//! [`crate::gemini_types`].

use crate::gemini_types::{
    FunctionCall, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration,
    FunctionResponse, GeminiTool, ToolConfig,
};
use serde_json::Value;

// ── FunctionDeclaration builder ─────────────────────────────────────────

/// Builder for [`FunctionDeclaration`].
#[derive(Debug, Clone)]
pub struct FunctionDeclarationBuilder {
    name: String,
    description: String,
    parameters: Value,
}

impl FunctionDeclarationBuilder {
    /// Start building a new function declaration with the given name.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }
    }

    /// Set a JSON Schema for the function parameters.
    #[must_use]
    pub fn parameters(mut self, schema: Value) -> Self {
        self.parameters = schema;
        self
    }

    /// Build the [`FunctionDeclaration`].
    #[must_use]
    pub fn build(self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: self.name,
            description: self.description,
            parameters: self.parameters,
        }
    }
}

// ── GeminiTool builder ──────────────────────────────────────────────────

/// Builder for a [`GeminiTool`] wrapping multiple function declarations.
#[derive(Debug, Clone, Default)]
pub struct GeminiToolBuilder {
    declarations: Vec<FunctionDeclaration>,
}

impl GeminiToolBuilder {
    /// Create a new empty tool builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a function declaration to this tool.
    #[must_use]
    pub fn add_function(mut self, decl: FunctionDeclaration) -> Self {
        self.declarations.push(decl);
        self
    }

    /// Build the [`GeminiTool`].
    #[must_use]
    pub fn build(self) -> GeminiTool {
        GeminiTool {
            function_declarations: self.declarations,
        }
    }
}

// ── ToolConfig builder ──────────────────────────────────────────────────

/// Builder for [`ToolConfig`].
#[derive(Debug, Clone)]
pub struct ToolConfigBuilder {
    mode: FunctionCallingMode,
    allowed: Option<Vec<String>>,
}

impl ToolConfigBuilder {
    /// Create with the specified calling mode.
    pub fn new(mode: FunctionCallingMode) -> Self {
        Self {
            mode,
            allowed: None,
        }
    }

    /// Restrict to specific function names.
    #[must_use]
    pub fn allowed_function_names(mut self, names: Vec<String>) -> Self {
        self.allowed = Some(names);
        self
    }

    /// Build the [`ToolConfig`].
    #[must_use]
    pub fn build(self) -> ToolConfig {
        ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: self.mode,
                allowed_function_names: self.allowed,
            },
        }
    }
}

// ── Validation helpers ──────────────────────────────────────────────────

/// Validate that a [`FunctionDeclaration`] has a non-empty name and description.
pub fn validate_declaration(decl: &FunctionDeclaration) -> Result<(), String> {
    if decl.name.is_empty() {
        return Err("function declaration name must not be empty".into());
    }
    if decl.description.is_empty() {
        return Err("function declaration description must not be empty".into());
    }
    Ok(())
}

/// Validate that a [`GeminiTool`] has at least one declaration and all are valid.
pub fn validate_tool(tool: &GeminiTool) -> Result<(), String> {
    if tool.function_declarations.is_empty() {
        return Err("tool must have at least one function declaration".into());
    }
    for decl in &tool.function_declarations {
        validate_declaration(decl)?;
    }
    Ok(())
}

/// Validate that a [`FunctionCallingConfig`] restricts to names that exist
/// in the provided tool declarations.
pub fn validate_config_against_tools(
    config: &FunctionCallingConfig,
    tools: &[GeminiTool],
) -> Result<(), String> {
    if let Some(allowed) = &config.allowed_function_names {
        let declared: std::collections::HashSet<&str> = tools
            .iter()
            .flat_map(|t| t.function_declarations.iter().map(|d| d.name.as_str()))
            .collect();
        for name in allowed {
            if !declared.contains(name.as_str()) {
                return Err(format!(
                    "allowed function name '{}' not found in any tool declaration",
                    name
                ));
            }
        }
    }
    Ok(())
}

// ── Extraction helpers ──────────────────────────────────────────────────

/// Extract all [`FunctionCall`] parts from a list of parts.
pub fn extract_function_calls(parts: &[crate::gemini_types::Part]) -> Vec<&FunctionCall> {
    parts
        .iter()
        .filter_map(|p| match p {
            crate::gemini_types::Part::FunctionCall(fc) => Some(fc),
            _ => None,
        })
        .collect()
}

/// Extract all [`FunctionResponse`] parts from a list of parts.
pub fn extract_function_responses(parts: &[crate::gemini_types::Part]) -> Vec<&FunctionResponse> {
    parts
        .iter()
        .filter_map(|p| match p {
            crate::gemini_types::Part::FunctionResponse(fr) => Some(fr),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn declaration_builder_defaults() {
        let decl = FunctionDeclarationBuilder::new("test", "A test function").build();
        assert_eq!(decl.name, "test");
        assert_eq!(decl.description, "A test function");
        assert_eq!(decl.parameters, json!({"type": "object", "properties": {}}));
    }

    #[test]
    fn declaration_builder_with_params() {
        let decl = FunctionDeclarationBuilder::new("search", "Search the web")
            .parameters(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }))
            .build();
        assert_eq!(decl.name, "search");
        assert!(decl.parameters["properties"]["query"].is_object());
    }

    #[test]
    fn tool_builder_single_function() {
        let tool = GeminiToolBuilder::new()
            .add_function(FunctionDeclarationBuilder::new("f1", "First function").build())
            .build();
        assert_eq!(tool.function_declarations.len(), 1);
    }

    #[test]
    fn tool_builder_multiple_functions() {
        let tool = GeminiToolBuilder::new()
            .add_function(FunctionDeclarationBuilder::new("f1", "First").build())
            .add_function(FunctionDeclarationBuilder::new("f2", "Second").build())
            .build();
        assert_eq!(tool.function_declarations.len(), 2);
    }

    #[test]
    fn tool_config_builder_auto_mode() {
        let cfg = ToolConfigBuilder::new(FunctionCallingMode::Auto).build();
        assert_eq!(cfg.function_calling_config.mode, FunctionCallingMode::Auto);
        assert!(cfg.function_calling_config.allowed_function_names.is_none());
    }

    #[test]
    fn tool_config_builder_with_allowed_names() {
        let cfg = ToolConfigBuilder::new(FunctionCallingMode::Any)
            .allowed_function_names(vec!["search".into(), "fetch".into()])
            .build();
        assert_eq!(cfg.function_calling_config.mode, FunctionCallingMode::Any);
        let names = cfg.function_calling_config.allowed_function_names.unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"search".into()));
    }

    #[test]
    fn validate_declaration_ok() {
        let decl = FunctionDeclarationBuilder::new("f", "desc").build();
        assert!(validate_declaration(&decl).is_ok());
    }

    #[test]
    fn validate_declaration_empty_name() {
        let decl = FunctionDeclaration {
            name: String::new(),
            description: "desc".into(),
            parameters: json!({}),
        };
        assert!(validate_declaration(&decl).is_err());
    }

    #[test]
    fn validate_declaration_empty_description() {
        let decl = FunctionDeclaration {
            name: "f".into(),
            description: String::new(),
            parameters: json!({}),
        };
        assert!(validate_declaration(&decl).is_err());
    }

    #[test]
    fn validate_tool_ok() {
        let tool = GeminiToolBuilder::new()
            .add_function(FunctionDeclarationBuilder::new("f", "desc").build())
            .build();
        assert!(validate_tool(&tool).is_ok());
    }

    #[test]
    fn validate_tool_empty() {
        let tool = GeminiTool {
            function_declarations: vec![],
        };
        assert!(validate_tool(&tool).is_err());
    }

    #[test]
    fn validate_config_against_tools_ok() {
        let tool = GeminiToolBuilder::new()
            .add_function(FunctionDeclarationBuilder::new("search", "s").build())
            .build();
        let config = FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into()]),
        };
        assert!(validate_config_against_tools(&config, &[tool]).is_ok());
    }

    #[test]
    fn validate_config_against_tools_missing_name() {
        let tool = GeminiToolBuilder::new()
            .add_function(FunctionDeclarationBuilder::new("search", "s").build())
            .build();
        let config = FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["nonexistent".into()]),
        };
        assert!(validate_config_against_tools(&config, &[tool]).is_err());
    }

    #[test]
    fn validate_config_no_allowed_names() {
        let config = FunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: None,
        };
        assert!(validate_config_against_tools(&config, &[]).is_ok());
    }

    #[test]
    fn extract_function_calls_mixed_parts() {
        use crate::gemini_types::Part;
        let parts = vec![
            Part::text("Hello"),
            Part::function_call("search", json!({"q": "rust"})),
            Part::text("More text"),
            Part::function_call("fetch", json!({"url": "http://example.com"})),
        ];
        let calls = extract_function_calls(&parts);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "search");
        assert_eq!(calls[1].name, "fetch");
    }

    #[test]
    fn extract_function_calls_none() {
        use crate::gemini_types::Part;
        let parts = vec![Part::text("Hello")];
        let calls = extract_function_calls(&parts);
        assert!(calls.is_empty());
    }

    #[test]
    fn extract_function_responses_mixed_parts() {
        use crate::gemini_types::Part;
        let parts = vec![
            Part::function_response("search", json!({"results": []})),
            Part::text("Done"),
            Part::function_response("fetch", json!({"body": "html"})),
        ];
        let responses = extract_function_responses(&parts);
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].name, "search");
        assert_eq!(responses[1].name, "fetch");
    }

    #[test]
    fn extract_function_responses_none() {
        use crate::gemini_types::Part;
        let parts = vec![Part::text("Hello")];
        let responses = extract_function_responses(&parts);
        assert!(responses.is_empty());
    }
}
