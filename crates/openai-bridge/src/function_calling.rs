// SPDX-License-Identifier: MIT OR Apache-2.0
//! Full function / tool calling types and helpers for the OpenAI bridge.
//!
//! Extends the base types in [`crate::openai_types`] with builder helpers,
//! parallel tool call assembly, and strict-mode schema wrappers.

use serde::{Deserialize, Serialize};

use crate::openai_types::{
    FunctionCall, FunctionDefinition, StreamToolCall, ToolCall, ToolDefinition,
};

// ── Tool choice control ─────────────────────────────────────────────

/// Controls which (if any) tool the model should call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    /// String mode: `"none"`, `"auto"`, or `"required"`.
    Mode(ToolChoiceMode),
    /// Force a specific function.
    Named(NamedToolChoice),
}

/// String-valued tool choice modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceMode {
    /// Model will not call any tool.
    None,
    /// Model decides whether to call a tool.
    Auto,
    /// Model must call at least one tool.
    Required,
}

/// Force the model to call a specific named function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedToolChoice {
    /// Always `"function"`.
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function to force.
    pub function: NamedFunction,
}

/// Function name reference inside a [`NamedToolChoice`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedFunction {
    /// Name of the function to call.
    pub name: String,
}

// ── Strict-mode function definition ─────────────────────────────────

/// A function definition with the `strict` flag for structured outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrictFunctionDefinition {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
    /// When `true`, the model guarantees the output matches the schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// A tool definition wrapping a [`StrictFunctionDefinition`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrictToolDefinition {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: StrictFunctionDefinition,
}

// ── Builders ────────────────────────────────────────────────────────

impl ToolDefinition {
    /// Build a function-type tool definition.
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".into(),
            function: FunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

impl ToolCall {
    /// Build a function-type tool call.
    pub fn function(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: arguments.into(),
            },
        }
    }
}

impl ToolChoice {
    /// Create an `"auto"` tool choice.
    pub fn auto() -> Self {
        Self::Mode(ToolChoiceMode::Auto)
    }

    /// Create a `"none"` tool choice.
    pub fn none() -> Self {
        Self::Mode(ToolChoiceMode::None)
    }

    /// Create a `"required"` tool choice.
    pub fn required() -> Self {
        Self::Mode(ToolChoiceMode::Required)
    }

    /// Force a specific function by name.
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(NamedToolChoice {
            tool_type: "function".into(),
            function: NamedFunction { name: name.into() },
        })
    }
}

// ── Parallel tool call assembly ─────────────────────────────────────

/// Assembles parallel tool calls from streaming delta fragments.
///
/// OpenAI streams multiple parallel tool calls interleaved by `index`.
/// This accumulator collects fragments and produces finished [`ToolCall`]s.
#[derive(Debug, Default)]
pub struct ParallelToolCallAssembler {
    builders: Vec<ToolCallInProgress>,
}

/// An in-progress tool call being assembled from stream fragments.
#[derive(Debug, Clone, Default)]
struct ToolCallInProgress {
    id: String,
    name: String,
    arguments: String,
}

impl ParallelToolCallAssembler {
    /// Create a new assembler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a stream tool call fragment.
    pub fn feed(&mut self, fragment: &StreamToolCall) {
        let idx = fragment.index as usize;
        while self.builders.len() <= idx {
            self.builders.push(ToolCallInProgress::default());
        }
        let builder = &mut self.builders[idx];

        if let Some(ref id) = fragment.id {
            builder.id = id.clone();
        }
        if let Some(ref func) = fragment.function {
            if let Some(ref name) = func.name {
                builder.name = name.clone();
            }
            if let Some(ref args) = func.arguments {
                builder.arguments.push_str(args);
            }
        }
    }

    /// Feed all tool call fragments from a delta.
    pub fn feed_all(&mut self, fragments: &[StreamToolCall]) {
        for f in fragments {
            self.feed(f);
        }
    }

    /// Finalize and return all assembled tool calls.
    pub fn finish(self) -> Vec<ToolCall> {
        self.builders
            .into_iter()
            .filter(|b| !b.id.is_empty())
            .map(|b| ToolCall {
                id: b.id,
                call_type: "function".into(),
                function: FunctionCall {
                    name: b.name,
                    arguments: b.arguments,
                },
            })
            .collect()
    }

    /// Number of tool calls being tracked.
    pub fn len(&self) -> usize {
        self.builders.len()
    }

    /// Whether no tool calls are being tracked.
    pub fn is_empty(&self) -> bool {
        self.builders.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai_types::StreamFunctionCall;

    // ── ToolChoice serde ───────────────────────────────────────────

    #[test]
    fn tool_choice_auto_serde() {
        let tc = ToolChoice::auto();
        let json = serde_json::to_string(&tc).unwrap();
        assert_eq!(json, r#""auto""#);
        let back: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tc);
    }

    #[test]
    fn tool_choice_none_serde() {
        let tc = ToolChoice::none();
        let json = serde_json::to_string(&tc).unwrap();
        assert_eq!(json, r#""none""#);
        let back: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tc);
    }

    #[test]
    fn tool_choice_required_serde() {
        let tc = ToolChoice::required();
        let json = serde_json::to_string(&tc).unwrap();
        assert_eq!(json, r#""required""#);
        let back: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tc);
    }

    #[test]
    fn tool_choice_named_serde() {
        let tc = ToolChoice::named("get_weather");
        let json = serde_json::to_string(&tc).unwrap();
        assert!(json.contains("get_weather"));
        assert!(json.contains("function"));
        let back: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tc);
    }

    // ── Builders ───────────────────────────────────────────────────

    #[test]
    fn tool_definition_builder() {
        let td = ToolDefinition::function(
            "search",
            "Search the web",
            serde_json::json!({"type": "object"}),
        );
        assert_eq!(td.tool_type, "function");
        assert_eq!(td.function.name, "search");
        assert_eq!(td.function.description, "Search the web");
    }

    #[test]
    fn tool_call_builder() {
        let tc = ToolCall::function("call_1", "search", r#"{"q":"rust"}"#);
        assert_eq!(tc.id, "call_1");
        assert_eq!(tc.call_type, "function");
        assert_eq!(tc.function.name, "search");
        assert_eq!(tc.function.arguments, r#"{"q":"rust"}"#);
    }

    // ── StrictFunctionDefinition serde ─────────────────────────────

    #[test]
    fn strict_function_definition_roundtrip() {
        let sfd = StrictFunctionDefinition {
            name: "calc".into(),
            description: "Calculator".into(),
            parameters: serde_json::json!({"type": "object"}),
            strict: Some(true),
        };
        let json = serde_json::to_string(&sfd).unwrap();
        assert!(json.contains("\"strict\":true"));
        let back: StrictFunctionDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(sfd, back);
    }

    #[test]
    fn strict_function_definition_omits_none_strict() {
        let sfd = StrictFunctionDefinition {
            name: "calc".into(),
            description: "Calculator".into(),
            parameters: serde_json::json!({}),
            strict: None,
        };
        let json = serde_json::to_string(&sfd).unwrap();
        assert!(!json.contains("strict"));
    }

    #[test]
    fn strict_tool_definition_roundtrip() {
        let std = StrictToolDefinition {
            tool_type: "function".into(),
            function: StrictFunctionDefinition {
                name: "f".into(),
                description: "d".into(),
                parameters: serde_json::json!({}),
                strict: Some(false),
            },
        };
        let json = serde_json::to_string(&std).unwrap();
        let back: StrictToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(std, back);
    }

    // ── ParallelToolCallAssembler ──────────────────────────────────

    #[test]
    fn assembler_single_tool_call() {
        let mut asm = ParallelToolCallAssembler::new();
        asm.feed(&StreamToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(StreamFunctionCall {
                name: Some("search".into()),
                arguments: Some(r#"{"q":"#.into()),
            }),
        });
        asm.feed(&StreamToolCall {
            index: 0,
            id: None,
            call_type: None,
            function: Some(StreamFunctionCall {
                name: None,
                arguments: Some(r#""rust"}"#.into()),
            }),
        });

        let calls = asm.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].function.name, "search");
        assert_eq!(calls[0].function.arguments, r#"{"q":"rust"}"#);
    }

    #[test]
    fn assembler_parallel_tool_calls() {
        let mut asm = ParallelToolCallAssembler::new();

        // First call start
        asm.feed(&StreamToolCall {
            index: 0,
            id: Some("call_a".into()),
            call_type: Some("function".into()),
            function: Some(StreamFunctionCall {
                name: Some("read".into()),
                arguments: Some(r#"{"p":"#.into()),
            }),
        });

        // Second call start
        asm.feed(&StreamToolCall {
            index: 1,
            id: Some("call_b".into()),
            call_type: Some("function".into()),
            function: Some(StreamFunctionCall {
                name: Some("write".into()),
                arguments: Some(r#"{"d":"#.into()),
            }),
        });

        // First call continue
        asm.feed(&StreamToolCall {
            index: 0,
            id: None,
            call_type: None,
            function: Some(StreamFunctionCall {
                name: None,
                arguments: Some(r#""a.txt"}"#.into()),
            }),
        });

        // Second call continue
        asm.feed(&StreamToolCall {
            index: 1,
            id: None,
            call_type: None,
            function: Some(StreamFunctionCall {
                name: None,
                arguments: Some(r#""hello"}"#.into()),
            }),
        });

        assert_eq!(asm.len(), 2);
        assert!(!asm.is_empty());

        let calls = asm.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call_a");
        assert_eq!(calls[0].function.name, "read");
        assert_eq!(calls[0].function.arguments, r#"{"p":"a.txt"}"#);
        assert_eq!(calls[1].id, "call_b");
        assert_eq!(calls[1].function.name, "write");
        assert_eq!(calls[1].function.arguments, r#"{"d":"hello"}"#);
    }

    #[test]
    fn assembler_feed_all() {
        let mut asm = ParallelToolCallAssembler::new();
        asm.feed_all(&[
            StreamToolCall {
                index: 0,
                id: Some("call_1".into()),
                call_type: Some("function".into()),
                function: Some(StreamFunctionCall {
                    name: Some("f".into()),
                    arguments: Some("{}".into()),
                }),
            },
            StreamToolCall {
                index: 1,
                id: Some("call_2".into()),
                call_type: Some("function".into()),
                function: Some(StreamFunctionCall {
                    name: Some("g".into()),
                    arguments: Some("{}".into()),
                }),
            },
        ]);

        let calls = asm.finish();
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn assembler_empty() {
        let asm = ParallelToolCallAssembler::new();
        assert!(asm.is_empty());
        assert_eq!(asm.len(), 0);
        let calls = asm.finish();
        assert!(calls.is_empty());
    }

    #[test]
    fn assembler_skips_empty_id_entries() {
        let mut asm = ParallelToolCallAssembler::new();
        // Feed a fragment at index 1 without ever setting index 0
        asm.feed(&StreamToolCall {
            index: 1,
            id: Some("call_x".into()),
            call_type: Some("function".into()),
            function: Some(StreamFunctionCall {
                name: Some("fn_x".into()),
                arguments: Some("{}".into()),
            }),
        });

        let calls = asm.finish();
        // Index 0 had no id, so it's filtered out
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_x");
    }

    #[test]
    fn named_tool_choice_debug() {
        let ntc = NamedToolChoice {
            tool_type: "function".into(),
            function: NamedFunction {
                name: "test_fn".into(),
            },
        };
        let debug = format!("{:?}", ntc);
        assert!(debug.contains("test_fn"));
    }

    #[test]
    fn tool_choice_mode_all_variants_serde() {
        for mode in [
            ToolChoiceMode::None,
            ToolChoiceMode::Auto,
            ToolChoiceMode::Required,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: ToolChoiceMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }
}
