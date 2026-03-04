// SPDX-License-Identifier: MIT OR Apache-2.0
//! Built-in Codex tool definitions.
//!
//! The OpenAI Responses API supports three built-in tool types:
//! - **Code Interpreter** — sandboxed code execution
//! - **File Search** — vector-store backed file search
//! - **Function** — user-defined function calling
//!
//! This module provides typed builders and the [`ToolDefinition`] enum that
//! unifies all three, matching the Codex API surface.

use serde::{Deserialize, Serialize};

// ── Tool definition enum ────────────────────────────────────────────────

/// A tool definition for the Codex Responses API.
///
/// Mirrors the `tools` array items accepted by the `/v1/responses` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolDefinition {
    /// Built-in code interpreter tool.
    CodeInterpreter(CodeInterpreterTool),
    /// Built-in file search tool.
    FileSearch(FileSearchTool),
    /// User-defined function tool.
    Function(FunctionTool),
}

// ── Code Interpreter ────────────────────────────────────────────────────

/// Configuration for the built-in Code Interpreter tool.
///
/// The code interpreter runs code in a sandboxed environment and returns
/// the output. Optionally, a container image can be specified.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CodeInterpreterTool {
    /// Container image for execution (e.g. `"python:3.12"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// Allowed file extensions for uploads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_extensions: Vec<String>,
}

impl CodeInterpreterTool {
    /// Create a new default code interpreter tool.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the container image.
    #[must_use]
    pub fn with_container(mut self, image: impl Into<String>) -> Self {
        self.container = Some(image.into());
        self
    }

    /// Set allowed file extensions.
    #[must_use]
    pub fn with_allowed_extensions(mut self, extensions: Vec<String>) -> Self {
        self.allowed_extensions = extensions;
        self
    }

    /// Convert to a [`ToolDefinition`].
    #[must_use]
    pub fn into_definition(self) -> ToolDefinition {
        ToolDefinition::CodeInterpreter(self)
    }
}

// ── File Search ─────────────────────────────────────────────────────────

/// Ranking options for file search results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileSearchRankingOptions {
    /// The ranker model to use (e.g. `"auto"`, `"default_2024_08_21"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranker: Option<String>,
    /// Minimum score threshold for results (0.0 to 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
}

/// Configuration for the built-in File Search tool.
///
/// Searches over vector stores attached to the assistant or thread.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FileSearchTool {
    /// IDs of vector stores to search.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vector_store_ids: Vec<String>,
    /// Maximum number of results to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_num_results: Option<u32>,
    /// Ranking options for search results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking_options: Option<FileSearchRankingOptions>,
}

impl FileSearchTool {
    /// Create a new default file search tool.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set vector store IDs to search.
    #[must_use]
    pub fn with_vector_stores(mut self, ids: Vec<String>) -> Self {
        self.vector_store_ids = ids;
        self
    }

    /// Set the maximum number of results.
    #[must_use]
    pub fn with_max_results(mut self, max: u32) -> Self {
        self.max_num_results = Some(max);
        self
    }

    /// Set ranking options.
    #[must_use]
    pub fn with_ranking(mut self, ranker: Option<String>, threshold: Option<f64>) -> Self {
        self.ranking_options = Some(FileSearchRankingOptions {
            ranker,
            score_threshold: threshold,
        });
        self
    }

    /// Convert to a [`ToolDefinition`].
    #[must_use]
    pub fn into_definition(self) -> ToolDefinition {
        ToolDefinition::FileSearch(self)
    }
}

// ── Function Tool ───────────────────────────────────────────────────────

/// A user-defined function tool.
///
/// Matches the OpenAI function calling schema with name, description,
/// JSON Schema parameters, and optional strict mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionTool {
    /// Function name (must match `^[a-zA-Z0-9_-]+$`).
    pub name: String,
    /// Human-readable description of what the function does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema describing the function parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    /// Whether to enforce strict schema validation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub strict: bool,
}

fn is_false(v: &bool) -> bool {
    !v
}

impl FunctionTool {
    /// Create a new function tool with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            parameters: None,
            strict: false,
        }
    }

    /// Set the description.
    #[must_use]
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the parameters JSON Schema.
    #[must_use]
    pub fn with_parameters(mut self, schema: serde_json::Value) -> Self {
        self.parameters = Some(schema);
        self
    }

    /// Enable strict schema validation.
    #[must_use]
    pub fn with_strict(mut self) -> Self {
        self.strict = true;
        self
    }

    /// Convert to a [`ToolDefinition`].
    #[must_use]
    pub fn into_definition(self) -> ToolDefinition {
        ToolDefinition::Function(self)
    }

    /// Convert to the SDK's `CodexTool::Function` variant.
    #[must_use]
    pub fn to_codex_tool(&self) -> abp_codex_sdk::dialect::CodexTool {
        abp_codex_sdk::dialect::CodexTool::Function {
            function: abp_codex_sdk::dialect::CodexFunctionDef {
                name: self.name.clone(),
                description: self.description.clone().unwrap_or_default(),
                parameters: self
                    .parameters
                    .clone()
                    .unwrap_or(serde_json::Value::Object(Default::default())),
            },
        }
    }
}

// ── Conversions ─────────────────────────────────────────────────────────

impl ToolDefinition {
    /// Convert to the SDK `CodexTool` enum.
    ///
    /// Code Interpreter and File Search map to their built-in SDK variants;
    /// Function maps to `CodexTool::Function`.
    #[must_use]
    pub fn to_codex_tool(&self) -> abp_codex_sdk::dialect::CodexTool {
        use abp_codex_sdk::dialect::CodexTool;
        match self {
            Self::CodeInterpreter(_) => CodexTool::CodeInterpreter {},
            Self::FileSearch(fs) => CodexTool::FileSearch {
                max_num_results: fs.max_num_results,
            },
            Self::Function(f) => f.to_codex_tool(),
        }
    }

    /// Create from the SDK `CodexTool` enum.
    #[must_use]
    pub fn from_codex_tool(tool: &abp_codex_sdk::dialect::CodexTool) -> Self {
        use abp_codex_sdk::dialect::CodexTool;
        match tool {
            CodexTool::CodeInterpreter {} => Self::CodeInterpreter(CodeInterpreterTool::default()),
            CodexTool::FileSearch { max_num_results } => Self::FileSearch(FileSearchTool {
                max_num_results: *max_num_results,
                ..FileSearchTool::default()
            }),
            CodexTool::Function { function } => Self::Function(FunctionTool {
                name: function.name.clone(),
                description: Some(function.description.clone()),
                parameters: Some(function.parameters.clone()),
                strict: false,
            }),
        }
    }

    /// Whether this is a built-in tool (code_interpreter or file_search).
    #[must_use]
    pub fn is_builtin(&self) -> bool {
        matches!(self, Self::CodeInterpreter(_) | Self::FileSearch(_))
    }

    /// The tool type name as a string slice.
    #[must_use]
    pub fn type_name(&self) -> &str {
        match self {
            Self::CodeInterpreter(_) => "code_interpreter",
            Self::FileSearch(_) => "file_search",
            Self::Function(_) => "function",
        }
    }
}

/// Convert a slice of [`ToolDefinition`]s to SDK `CodexTool`s.
#[must_use]
pub fn to_codex_tools(tools: &[ToolDefinition]) -> Vec<abp_codex_sdk::dialect::CodexTool> {
    tools.iter().map(|t| t.to_codex_tool()).collect()
}

/// Convert a slice of SDK `CodexTool`s to [`ToolDefinition`]s.
#[must_use]
pub fn from_codex_tools(tools: &[abp_codex_sdk::dialect::CodexTool]) -> Vec<ToolDefinition> {
    tools.iter().map(ToolDefinition::from_codex_tool).collect()
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── CodeInterpreterTool tests ───────────────────────────────────────

    #[test]
    fn code_interpreter_default() {
        let tool = CodeInterpreterTool::new();
        assert!(tool.container.is_none());
        assert!(tool.allowed_extensions.is_empty());
    }

    #[test]
    fn code_interpreter_with_container() {
        let tool = CodeInterpreterTool::new().with_container("python:3.12");
        assert_eq!(tool.container.as_deref(), Some("python:3.12"));
    }

    #[test]
    fn code_interpreter_with_extensions() {
        let tool =
            CodeInterpreterTool::new().with_allowed_extensions(vec!["py".into(), "js".into()]);
        assert_eq!(tool.allowed_extensions, vec!["py", "js"]);
    }

    #[test]
    fn code_interpreter_into_definition() {
        let def = CodeInterpreterTool::new().into_definition();
        assert!(matches!(def, ToolDefinition::CodeInterpreter(_)));
        assert!(def.is_builtin());
        assert_eq!(def.type_name(), "code_interpreter");
    }

    #[test]
    fn code_interpreter_serde_roundtrip() {
        let def = CodeInterpreterTool::new()
            .with_container("node:20")
            .into_definition();
        let json = serde_json::to_string(&def).unwrap();
        assert!(json.contains("code_interpreter"));
        let decoded: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(def, decoded);
    }

    // ── FileSearchTool tests ────────────────────────────────────────────

    #[test]
    fn file_search_default() {
        let tool = FileSearchTool::new();
        assert!(tool.vector_store_ids.is_empty());
        assert!(tool.max_num_results.is_none());
        assert!(tool.ranking_options.is_none());
    }

    #[test]
    fn file_search_with_vector_stores() {
        let tool = FileSearchTool::new().with_vector_stores(vec!["vs_abc".into(), "vs_def".into()]);
        assert_eq!(tool.vector_store_ids, vec!["vs_abc", "vs_def"]);
    }

    #[test]
    fn file_search_with_max_results() {
        let tool = FileSearchTool::new().with_max_results(10);
        assert_eq!(tool.max_num_results, Some(10));
    }

    #[test]
    fn file_search_with_ranking() {
        let tool = FileSearchTool::new().with_ranking(Some("auto".into()), Some(0.5));
        let opts = tool.ranking_options.unwrap();
        assert_eq!(opts.ranker.as_deref(), Some("auto"));
        assert_eq!(opts.score_threshold, Some(0.5));
    }

    #[test]
    fn file_search_into_definition() {
        let def = FileSearchTool::new().into_definition();
        assert!(matches!(def, ToolDefinition::FileSearch(_)));
        assert!(def.is_builtin());
        assert_eq!(def.type_name(), "file_search");
    }

    #[test]
    fn file_search_serde_roundtrip() {
        let def = FileSearchTool::new()
            .with_vector_stores(vec!["vs_1".into()])
            .with_max_results(5)
            .into_definition();
        let json = serde_json::to_string(&def).unwrap();
        assert!(json.contains("file_search"));
        let decoded: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(def, decoded);
    }

    // ── FunctionTool tests ──────────────────────────────────────────────

    #[test]
    fn function_tool_new() {
        let tool = FunctionTool::new("get_weather");
        assert_eq!(tool.name, "get_weather");
        assert!(tool.description.is_none());
        assert!(tool.parameters.is_none());
        assert!(!tool.strict);
    }

    #[test]
    fn function_tool_with_description() {
        let tool = FunctionTool::new("search").with_description("Search the web");
        assert_eq!(tool.description.as_deref(), Some("Search the web"));
    }

    #[test]
    fn function_tool_with_parameters() {
        let schema = json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        });
        let tool = FunctionTool::new("search").with_parameters(schema.clone());
        assert_eq!(tool.parameters, Some(schema));
    }

    #[test]
    fn function_tool_with_strict() {
        let tool = FunctionTool::new("exec").with_strict();
        assert!(tool.strict);
    }

    #[test]
    fn function_tool_into_definition() {
        let def = FunctionTool::new("my_fn").into_definition();
        assert!(matches!(def, ToolDefinition::Function(_)));
        assert!(!def.is_builtin());
        assert_eq!(def.type_name(), "function");
    }

    #[test]
    fn function_tool_serde_roundtrip() {
        let def = FunctionTool::new("calc")
            .with_description("Calculate expression")
            .with_parameters(json!({"type": "object", "properties": {"expr": {"type": "string"}}}))
            .with_strict()
            .into_definition();
        let json_str = serde_json::to_string(&def).unwrap();
        assert!(json_str.contains("function"));
        assert!(json_str.contains("calc"));
        let decoded: ToolDefinition = serde_json::from_str(&json_str).unwrap();
        assert_eq!(def, decoded);
    }

    #[test]
    fn function_tool_strict_omitted_when_false() {
        let def = FunctionTool::new("test").into_definition();
        let json = serde_json::to_string(&def).unwrap();
        assert!(!json.contains("strict"));
    }

    // ── ToolDefinition conversion tests ─────────────────────────────────

    #[test]
    fn to_codex_tool_code_interpreter() {
        let def = CodeInterpreterTool::new().into_definition();
        let sdk = def.to_codex_tool();
        assert!(matches!(
            sdk,
            abp_codex_sdk::dialect::CodexTool::CodeInterpreter {}
        ));
    }

    #[test]
    fn to_codex_tool_file_search() {
        let def = FileSearchTool::new().into_definition();
        let sdk = def.to_codex_tool();
        assert!(matches!(
            sdk,
            abp_codex_sdk::dialect::CodexTool::FileSearch { .. }
        ));
    }

    #[test]
    fn to_codex_tool_function() {
        let def = FunctionTool::new("shell")
            .with_description("Run a shell command")
            .into_definition();
        let sdk = def.to_codex_tool();
        match sdk {
            abp_codex_sdk::dialect::CodexTool::Function { function } => {
                assert_eq!(function.name, "shell");
                assert_eq!(function.description.as_str(), "Run a shell command");
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn from_codex_tool_code_interpreter() {
        let sdk = abp_codex_sdk::dialect::CodexTool::CodeInterpreter {};
        let def = ToolDefinition::from_codex_tool(&sdk);
        assert!(matches!(def, ToolDefinition::CodeInterpreter(_)));
    }

    #[test]
    fn from_codex_tool_file_search() {
        let sdk = abp_codex_sdk::dialect::CodexTool::FileSearch {
            max_num_results: None,
        };
        let def = ToolDefinition::from_codex_tool(&sdk);
        assert!(matches!(def, ToolDefinition::FileSearch(_)));
    }

    #[test]
    fn from_codex_tool_function() {
        let sdk = abp_codex_sdk::dialect::CodexTool::Function {
            function: abp_codex_sdk::dialect::CodexFunctionDef {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: serde_json::Value::Object(Default::default()),
            },
        };
        let def = ToolDefinition::from_codex_tool(&sdk);
        match def {
            ToolDefinition::Function(f) => {
                assert_eq!(f.name, "read_file");
                assert_eq!(f.description.as_deref(), Some("Read a file"));
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // ── Batch conversion tests ──────────────────────────────────────────

    #[test]
    fn to_codex_tools_batch() {
        let defs = vec![
            CodeInterpreterTool::new().into_definition(),
            FileSearchTool::new().into_definition(),
            FunctionTool::new("test").into_definition(),
        ];
        let sdk_tools = to_codex_tools(&defs);
        assert_eq!(sdk_tools.len(), 3);
    }

    #[test]
    fn from_codex_tools_batch() {
        let sdk_tools = vec![
            abp_codex_sdk::dialect::CodexTool::CodeInterpreter {},
            abp_codex_sdk::dialect::CodexTool::FileSearch {
                max_num_results: None,
            },
        ];
        let defs = from_codex_tools(&sdk_tools);
        assert_eq!(defs.len(), 2);
        assert!(defs[0].is_builtin());
        assert!(defs[1].is_builtin());
    }

    #[test]
    fn roundtrip_to_and_from_codex_tools() {
        let original = vec![
            CodeInterpreterTool::new().into_definition(),
            FunctionTool::new("shell")
                .with_description("Run command")
                .into_definition(),
        ];
        let sdk = to_codex_tools(&original);
        let back = from_codex_tools(&sdk);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].type_name(), "code_interpreter");
        assert_eq!(back[1].type_name(), "function");
    }
}
