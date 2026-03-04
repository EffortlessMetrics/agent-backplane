// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kimi built-in tool definitions.
//!
//! Kimi supports several built-in tools that can be activated by including
//! them in the `tools` array of a chat completions request. These tools run
//! server-side within the Kimi platform.
//!
//! - [`SearchTool`] — web search via `$web_search`
//! - [`FileTool`] — file analysis via `$file_tool`
//! - [`CodeTool`] — code execution via `$code_tool`
//! - [`BrowserTool`] — web browsing via `$browser`

use serde::{Deserialize, Serialize};

// ── SearchTool ──────────────────────────────────────────────────────────

/// Kimi built-in web search tool (`$web_search`).
///
/// When included in a request, Kimi performs server-side web searches and
/// injects citation references into the response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchTool {
    /// Whether the search tool is enabled.
    pub enabled: bool,
    /// Optional search scope hint (e.g. `"general"`, `"academic"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_scope: Option<String>,
}

impl Default for SearchTool {
    fn default() -> Self {
        Self {
            enabled: true,
            search_scope: None,
        }
    }
}

impl SearchTool {
    /// Create an enabled search tool with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a search tool with a specific scope.
    #[must_use]
    pub fn with_scope(scope: impl Into<String>) -> Self {
        Self {
            enabled: true,
            search_scope: Some(scope.into()),
        }
    }

    /// The Kimi built-in function name for web search.
    #[must_use]
    pub const fn function_name() -> &'static str {
        "$web_search"
    }

    /// Convert to the Kimi built-in tool wire format.
    #[must_use]
    pub fn to_builtin_tool(&self) -> abp_kimi_sdk::dialect::KimiBuiltinTool {
        abp_kimi_sdk::dialect::KimiBuiltinTool {
            tool_type: "builtin_function".into(),
            function: abp_kimi_sdk::dialect::KimiBuiltinFunction {
                name: Self::function_name().into(),
            },
        }
    }
}

// ── FileTool ────────────────────────────────────────────────────────────

/// Kimi built-in file analysis tool (`$file_tool`).
///
/// Enables server-side file analysis for documents uploaded via the Kimi
/// Files API. The model can extract text, summarize, and answer questions
/// about uploaded files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileTool {
    /// Whether the file tool is enabled.
    pub enabled: bool,
    /// File IDs to make available for analysis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_ids: Vec<String>,
}

impl Default for FileTool {
    fn default() -> Self {
        Self {
            enabled: true,
            file_ids: Vec::new(),
        }
    }
}

impl FileTool {
    /// Create an enabled file tool with no pre-loaded files.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a file tool with specific file IDs.
    #[must_use]
    pub fn with_files(file_ids: Vec<String>) -> Self {
        Self {
            enabled: true,
            file_ids,
        }
    }

    /// The Kimi built-in function name for file analysis.
    #[must_use]
    pub const fn function_name() -> &'static str {
        "$file_tool"
    }

    /// Convert to the Kimi built-in tool wire format.
    #[must_use]
    pub fn to_builtin_tool(&self) -> abp_kimi_sdk::dialect::KimiBuiltinTool {
        abp_kimi_sdk::dialect::KimiBuiltinTool {
            tool_type: "builtin_function".into(),
            function: abp_kimi_sdk::dialect::KimiBuiltinFunction {
                name: Self::function_name().into(),
            },
        }
    }
}

// ── CodeTool ────────────────────────────────────────────────────────────

/// Kimi built-in code execution tool (`$code_tool`).
///
/// Enables server-side code execution (sandboxed). The model can generate
/// and run code to answer mathematical, data-processing, or programming
/// questions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeTool {
    /// Whether the code tool is enabled.
    pub enabled: bool,
    /// Allowed languages (empty = all supported).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
}

impl Default for CodeTool {
    fn default() -> Self {
        Self {
            enabled: true,
            languages: Vec::new(),
        }
    }
}

impl CodeTool {
    /// Create an enabled code tool with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a code tool restricted to specific languages.
    #[must_use]
    pub fn with_languages(languages: Vec<String>) -> Self {
        Self {
            enabled: true,
            languages,
        }
    }

    /// The Kimi built-in function name for code execution.
    #[must_use]
    pub const fn function_name() -> &'static str {
        "$code_tool"
    }

    /// Convert to the Kimi built-in tool wire format.
    #[must_use]
    pub fn to_builtin_tool(&self) -> abp_kimi_sdk::dialect::KimiBuiltinTool {
        abp_kimi_sdk::dialect::KimiBuiltinTool {
            tool_type: "builtin_function".into(),
            function: abp_kimi_sdk::dialect::KimiBuiltinFunction {
                name: Self::function_name().into(),
            },
        }
    }
}

// ── BrowserTool ─────────────────────────────────────────────────────────

/// Kimi built-in web browsing tool (`$browser`).
///
/// Enables server-side web browsing. The model can visit URLs, read page
/// content, and extract information from web pages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrowserTool {
    /// Whether the browser tool is enabled.
    pub enabled: bool,
}

impl Default for BrowserTool {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl BrowserTool {
    /// Create an enabled browser tool.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The Kimi built-in function name for web browsing.
    #[must_use]
    pub const fn function_name() -> &'static str {
        "$browser"
    }

    /// Convert to the Kimi built-in tool wire format.
    #[must_use]
    pub fn to_builtin_tool(&self) -> abp_kimi_sdk::dialect::KimiBuiltinTool {
        abp_kimi_sdk::dialect::KimiBuiltinTool {
            tool_type: "builtin_function".into(),
            function: abp_kimi_sdk::dialect::KimiBuiltinFunction {
                name: "$browser".into(),
            },
        }
    }
}

// ── Helper: collect all enabled built-in tools ──────────────────────────

/// Configuration for all Kimi built-in tools.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BuiltinTools {
    /// Web search tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchTool>,
    /// File analysis tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileTool>,
    /// Code execution tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<CodeTool>,
    /// Web browser tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserTool>,
}

impl BuiltinTools {
    /// Create a new configuration with no tools enabled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable all built-in tools with default settings.
    #[must_use]
    pub fn all() -> Self {
        Self {
            search: Some(SearchTool::new()),
            file: Some(FileTool::new()),
            code: Some(CodeTool::new()),
            browser: Some(BrowserTool::new()),
        }
    }

    /// Collect enabled tools into Kimi built-in tool wire format.
    #[must_use]
    pub fn to_builtin_tools(&self) -> Vec<abp_kimi_sdk::dialect::KimiBuiltinTool> {
        let mut tools = Vec::new();
        if let Some(s) = &self.search {
            if s.enabled {
                tools.push(s.to_builtin_tool());
            }
        }
        if let Some(f) = &self.file {
            if f.enabled {
                tools.push(f.to_builtin_tool());
            }
        }
        if let Some(c) = &self.code {
            if c.enabled {
                tools.push(c.to_builtin_tool());
            }
        }
        if let Some(b) = &self.browser {
            if b.enabled {
                tools.push(b.to_builtin_tool());
            }
        }
        tools
    }

    /// Returns the number of enabled tools.
    #[must_use]
    pub fn enabled_count(&self) -> usize {
        self.to_builtin_tools().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SearchTool ──────────────────────────────────────────────────────

    #[test]
    fn search_tool_default_enabled() {
        let tool = SearchTool::new();
        assert!(tool.enabled);
        assert!(tool.search_scope.is_none());
    }

    #[test]
    fn search_tool_with_scope() {
        let tool = SearchTool::with_scope("academic");
        assert!(tool.enabled);
        assert_eq!(tool.search_scope.as_deref(), Some("academic"));
    }

    #[test]
    fn search_tool_function_name() {
        assert_eq!(SearchTool::function_name(), "$web_search");
    }

    #[test]
    fn search_tool_to_builtin() {
        let tool = SearchTool::new();
        let bt = tool.to_builtin_tool();
        assert_eq!(bt.tool_type, "builtin_function");
        assert_eq!(bt.function.name, "$web_search");
    }

    #[test]
    fn search_tool_serde_roundtrip() {
        let tool = SearchTool::with_scope("general");
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: SearchTool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tool);
    }

    // ── FileTool ────────────────────────────────────────────────────────

    #[test]
    fn file_tool_default_enabled() {
        let tool = FileTool::new();
        assert!(tool.enabled);
        assert!(tool.file_ids.is_empty());
    }

    #[test]
    fn file_tool_with_files() {
        let tool = FileTool::with_files(vec!["file-1".into(), "file-2".into()]);
        assert_eq!(tool.file_ids.len(), 2);
        assert_eq!(tool.file_ids[0], "file-1");
    }

    #[test]
    fn file_tool_function_name() {
        assert_eq!(FileTool::function_name(), "$file_tool");
    }

    #[test]
    fn file_tool_to_builtin() {
        let tool = FileTool::new();
        let bt = tool.to_builtin_tool();
        assert_eq!(bt.function.name, "$file_tool");
    }

    #[test]
    fn file_tool_serde_roundtrip() {
        let tool = FileTool::with_files(vec!["file-abc".into()]);
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: FileTool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tool);
    }

    #[test]
    fn file_tool_empty_files_omitted_in_json() {
        let tool = FileTool::new();
        let json = serde_json::to_string(&tool).unwrap();
        assert!(!json.contains("file_ids"));
    }

    // ── CodeTool ────────────────────────────────────────────────────────

    #[test]
    fn code_tool_default_enabled() {
        let tool = CodeTool::new();
        assert!(tool.enabled);
        assert!(tool.languages.is_empty());
    }

    #[test]
    fn code_tool_with_languages() {
        let tool = CodeTool::with_languages(vec!["python".into(), "javascript".into()]);
        assert_eq!(tool.languages.len(), 2);
    }

    #[test]
    fn code_tool_function_name() {
        assert_eq!(CodeTool::function_name(), "$code_tool");
    }

    #[test]
    fn code_tool_to_builtin() {
        let tool = CodeTool::new();
        let bt = tool.to_builtin_tool();
        assert_eq!(bt.function.name, "$code_tool");
    }

    #[test]
    fn code_tool_serde_roundtrip() {
        let tool = CodeTool::with_languages(vec!["python".into()]);
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: CodeTool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tool);
    }

    // ── BrowserTool ─────────────────────────────────────────────────────

    #[test]
    fn browser_tool_default_enabled() {
        let tool = BrowserTool::new();
        assert!(tool.enabled);
    }

    #[test]
    fn browser_tool_function_name() {
        assert_eq!(BrowserTool::function_name(), "$browser");
    }

    #[test]
    fn browser_tool_to_builtin() {
        let tool = BrowserTool::new();
        let bt = tool.to_builtin_tool();
        assert_eq!(bt.tool_type, "builtin_function");
        assert_eq!(bt.function.name, "$browser");
    }

    #[test]
    fn browser_tool_serde_roundtrip() {
        let tool = BrowserTool::new();
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: BrowserTool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tool);
    }

    // ── BuiltinTools collection ─────────────────────────────────────────

    #[test]
    fn builtin_tools_default_empty() {
        let bt = BuiltinTools::new();
        assert_eq!(bt.enabled_count(), 0);
        assert!(bt.to_builtin_tools().is_empty());
    }

    #[test]
    fn builtin_tools_all_enables_four() {
        let bt = BuiltinTools::all();
        assert_eq!(bt.enabled_count(), 4);
    }

    #[test]
    fn builtin_tools_selective_enable() {
        let bt = BuiltinTools {
            search: Some(SearchTool::new()),
            file: None,
            code: Some(CodeTool::new()),
            browser: None,
        };
        assert_eq!(bt.enabled_count(), 2);
        let tools = bt.to_builtin_tools();
        assert_eq!(tools[0].function.name, "$web_search");
        assert_eq!(tools[1].function.name, "$code_tool");
    }

    #[test]
    fn builtin_tools_disabled_tool_excluded() {
        let bt = BuiltinTools {
            search: Some(SearchTool {
                enabled: false,
                search_scope: None,
            }),
            file: None,
            code: None,
            browser: Some(BrowserTool::new()),
        };
        assert_eq!(bt.enabled_count(), 1);
        assert_eq!(bt.to_builtin_tools()[0].function.name, "$browser");
    }

    #[test]
    fn builtin_tools_serde_roundtrip() {
        let bt = BuiltinTools::all();
        let json = serde_json::to_string(&bt).unwrap();
        let parsed: BuiltinTools = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, bt);
    }

    #[test]
    fn builtin_tools_empty_none_fields_omitted() {
        let bt = BuiltinTools::new();
        let json = serde_json::to_string(&bt).unwrap();
        assert_eq!(json, "{}");
    }
}
