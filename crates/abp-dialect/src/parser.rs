// SPDX-License-Identifier: MIT OR Apache-2.0
//! Strict dialect parsing for vendor-specific request JSON.
//!
//! Each [`DialectParser`] implementation validates that a raw JSON value
//! conforms to the vendor's request schema — required fields, correct
//! types, valid enum values, tool definitions, streaming flags, and model
//! names — returning rich [`ParseError`]s with field paths, expected
//! types, and actual values.

use serde_json::Value;
use std::fmt;

use crate::Dialect;

// ── ParseError ──────────────────────────────────────────────────────────

/// Rich parse error with field path context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// JSON-pointer-style path to the problematic field (e.g. `messages[0].role`).
    pub field_path: String,
    /// What was expected (e.g. `"string"`, `"one of: user, assistant"`).
    pub expected: String,
    /// What was actually found (e.g. `"number: 42"`).
    pub actual: String,
    /// Machine-readable error code.
    pub code: ParseErrorCode,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: expected {}, got {} ({})",
            self.field_path, self.expected, self.actual, self.code
        )
    }
}

impl std::error::Error for ParseError {}

/// Machine-readable error codes for parse failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParseErrorCode {
    /// A required field is missing.
    MissingField,
    /// A field has the wrong JSON type.
    InvalidType,
    /// A string value is not in the allowed set.
    InvalidEnumValue,
    /// An array is empty when at least one element is required.
    EmptyArray,
    /// A nested structure is malformed.
    InvalidStructure,
}

impl fmt::Display for ParseErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField => f.write_str("missing_field"),
            Self::InvalidType => f.write_str("invalid_type"),
            Self::InvalidEnumValue => f.write_str("invalid_enum_value"),
            Self::EmptyArray => f.write_str("empty_array"),
            Self::InvalidStructure => f.write_str("invalid_structure"),
        }
    }
}

/// Aggregate result of strict parsing.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// All errors found during parsing.
    pub errors: Vec<ParseError>,
}

impl ParseResult {
    /// `true` when parsing found no errors.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

// ── DialectParser trait ─────────────────────────────────────────────────

/// Trait for strict dialect-specific request parsing.
///
/// Implementations validate that a raw JSON [`Value`] conforms to the
/// vendor's request schema.
pub trait DialectParser: Send + Sync {
    /// Which dialect this parser validates.
    fn dialect(&self) -> Dialect;

    /// Strictly parse and validate `request`.
    fn parse(&self, request: &Value) -> ParseResult;
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn value_type_name(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => format!("bool: {b}"),
        Value::Number(n) => format!("number: {n}"),
        Value::String(s) => {
            if s.len() > 32 {
                format!("string: \"{}...\"", &s[..32])
            } else {
                format!("string: \"{s}\"")
            }
        }
        Value::Array(a) => format!("array(len={})", a.len()),
        Value::Object(o) => format!("object(keys={})", o.len()),
    }
}

fn require_string_field(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    errors: &mut Vec<ParseError>,
) {
    match obj.get(field) {
        None => errors.push(ParseError {
            field_path: field.into(),
            expected: "string".into(),
            actual: "missing".into(),
            code: ParseErrorCode::MissingField,
        }),
        Some(Value::String(s)) if s.is_empty() => errors.push(ParseError {
            field_path: field.into(),
            expected: "non-empty string".into(),
            actual: "empty string".into(),
            code: ParseErrorCode::InvalidType,
        }),
        Some(Value::String(_)) => {}
        Some(other) => errors.push(ParseError {
            field_path: field.into(),
            expected: "string".into(),
            actual: value_type_name(other),
            code: ParseErrorCode::InvalidType,
        }),
    }
}

fn require_array_field(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    errors: &mut Vec<ParseError>,
) -> Option<Vec<Value>> {
    match obj.get(field) {
        None => {
            errors.push(ParseError {
                field_path: field.into(),
                expected: "array".into(),
                actual: "missing".into(),
                code: ParseErrorCode::MissingField,
            });
            None
        }
        Some(Value::Array(arr)) => Some(arr.clone()),
        Some(other) => {
            errors.push(ParseError {
                field_path: field.into(),
                expected: "array".into(),
                actual: value_type_name(other),
                code: ParseErrorCode::InvalidType,
            });
            None
        }
    }
}

fn check_stream_field(obj: &serde_json::Map<String, Value>, errors: &mut Vec<ParseError>) {
    if let Some(v) = obj.get("stream") {
        if !v.is_boolean() {
            errors.push(ParseError {
                field_path: "stream".into(),
                expected: "boolean".into(),
                actual: value_type_name(v),
                code: ParseErrorCode::InvalidType,
            });
        }
    }
}

fn check_model_present(obj: &serde_json::Map<String, Value>, errors: &mut Vec<ParseError>) {
    require_string_field(obj, "model", errors);
}

fn validate_tool_definitions(tools: &[Value], prefix: &str, errors: &mut Vec<ParseError>) {
    for (i, tool) in tools.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        let Some(obj) = tool.as_object() else {
            errors.push(ParseError {
                field_path: path,
                expected: "object".into(),
                actual: value_type_name(tool),
                code: ParseErrorCode::InvalidType,
            });
            continue;
        };
        // Most dialects require type and function sub-object for tools.
        if let Some(Value::String(t)) = obj.get("type") {
            if t == "function" {
                match obj.get("function") {
                    Some(Value::Object(func)) => {
                        if func.get("name").and_then(Value::as_str).is_none() {
                            errors.push(ParseError {
                                field_path: format!("{path}.function.name"),
                                expected: "non-empty string".into(),
                                actual: func.get("name").map_or("missing".into(), value_type_name),
                                code: if func.contains_key("name") {
                                    ParseErrorCode::InvalidType
                                } else {
                                    ParseErrorCode::MissingField
                                },
                            });
                        }
                    }
                    Some(other) => {
                        errors.push(ParseError {
                            field_path: format!("{path}.function"),
                            expected: "object".into(),
                            actual: value_type_name(other),
                            code: ParseErrorCode::InvalidType,
                        });
                    }
                    None => {
                        errors.push(ParseError {
                            field_path: format!("{path}.function"),
                            expected: "object".into(),
                            actual: "missing".into(),
                            code: ParseErrorCode::MissingField,
                        });
                    }
                }
            }
        }
    }
}

fn check_roles(msgs: &[Value], prefix: &str, valid_roles: &[&str], errors: &mut Vec<ParseError>) {
    for (i, msg) in msgs.iter().enumerate() {
        let path = format!("{prefix}[{i}].role");
        match msg.get("role") {
            None => errors.push(ParseError {
                field_path: path,
                expected: format!("one of: {}", valid_roles.join(", ")),
                actual: "missing".into(),
                code: ParseErrorCode::MissingField,
            }),
            Some(Value::String(role)) => {
                if !valid_roles.contains(&role.as_str()) {
                    errors.push(ParseError {
                        field_path: path,
                        expected: format!("one of: {}", valid_roles.join(", ")),
                        actual: format!("string: \"{role}\""),
                        code: ParseErrorCode::InvalidEnumValue,
                    });
                }
            }
            Some(other) => errors.push(ParseError {
                field_path: path,
                expected: "string".into(),
                actual: value_type_name(other),
                code: ParseErrorCode::InvalidType,
            }),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OpenAI dialect parser
// ═══════════════════════════════════════════════════════════════════════

const OPENAI_ROLES: &[&str] = &[
    "system",
    "user",
    "assistant",
    "tool",
    "function",
    "developer",
];

/// Strict parser for OpenAI ChatCompletion request schema.
#[derive(Debug, Default)]
pub struct OpenAiParser;

impl DialectParser for OpenAiParser {
    fn dialect(&self) -> Dialect {
        Dialect::OpenAi
    }

    fn parse(&self, request: &Value) -> ParseResult {
        let mut errors = Vec::new();
        let Some(obj) = request.as_object() else {
            errors.push(ParseError {
                field_path: "<root>".into(),
                expected: "object".into(),
                actual: value_type_name(request),
                code: ParseErrorCode::InvalidType,
            });
            return ParseResult { errors };
        };

        check_model_present(obj, &mut errors);
        check_stream_field(obj, &mut errors);

        if let Some(msgs) = require_array_field(obj, "messages", &mut errors) {
            if msgs.is_empty() {
                errors.push(ParseError {
                    field_path: "messages".into(),
                    expected: "non-empty array".into(),
                    actual: "empty array".into(),
                    code: ParseErrorCode::EmptyArray,
                });
            } else {
                check_roles(&msgs, "messages", OPENAI_ROLES, &mut errors);
            }
        }

        if let Some(Value::Array(tools)) = obj.get("tools") {
            validate_tool_definitions(tools, "tools", &mut errors);
        } else if let Some(v) = obj.get("tools") {
            errors.push(ParseError {
                field_path: "tools".into(),
                expected: "array".into(),
                actual: value_type_name(v),
                code: ParseErrorCode::InvalidType,
            });
        }

        // temperature must be a number if present
        if let Some(v) = obj.get("temperature") {
            if !v.is_number() {
                errors.push(ParseError {
                    field_path: "temperature".into(),
                    expected: "number".into(),
                    actual: value_type_name(v),
                    code: ParseErrorCode::InvalidType,
                });
            }
        }

        ParseResult { errors }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Claude dialect parser
// ═══════════════════════════════════════════════════════════════════════

const CLAUDE_ROLES: &[&str] = &["user", "assistant"];

/// Strict parser for Anthropic Claude Messages API request schema.
#[derive(Debug, Default)]
pub struct ClaudeParser;

impl DialectParser for ClaudeParser {
    fn dialect(&self) -> Dialect {
        Dialect::Claude
    }

    fn parse(&self, request: &Value) -> ParseResult {
        let mut errors = Vec::new();
        let Some(obj) = request.as_object() else {
            errors.push(ParseError {
                field_path: "<root>".into(),
                expected: "object".into(),
                actual: value_type_name(request),
                code: ParseErrorCode::InvalidType,
            });
            return ParseResult { errors };
        };

        check_model_present(obj, &mut errors);
        check_stream_field(obj, &mut errors);

        // max_tokens is required for Claude
        match obj.get("max_tokens") {
            None => errors.push(ParseError {
                field_path: "max_tokens".into(),
                expected: "positive integer".into(),
                actual: "missing".into(),
                code: ParseErrorCode::MissingField,
            }),
            Some(v) if !v.is_number() => errors.push(ParseError {
                field_path: "max_tokens".into(),
                expected: "positive integer".into(),
                actual: value_type_name(v),
                code: ParseErrorCode::InvalidType,
            }),
            _ => {}
        }

        if let Some(msgs) = require_array_field(obj, "messages", &mut errors) {
            if msgs.is_empty() {
                errors.push(ParseError {
                    field_path: "messages".into(),
                    expected: "non-empty array".into(),
                    actual: "empty array".into(),
                    code: ParseErrorCode::EmptyArray,
                });
            } else {
                check_roles(&msgs, "messages", CLAUDE_ROLES, &mut errors);
                // content must be string or array of content blocks
                for (i, msg) in msgs.iter().enumerate() {
                    if let Some(content) = msg.get("content") {
                        if !content.is_string() && !content.is_array() {
                            errors.push(ParseError {
                                field_path: format!("messages[{i}].content"),
                                expected: "string or array".into(),
                                actual: value_type_name(content),
                                code: ParseErrorCode::InvalidType,
                            });
                        }
                        if let Some(blocks) = content.as_array() {
                            for (j, block) in blocks.iter().enumerate() {
                                if block.get("type").and_then(Value::as_str).is_none() {
                                    errors.push(ParseError {
                                        field_path: format!("messages[{i}].content[{j}].type"),
                                        expected: "string (content block type)".into(),
                                        actual: block
                                            .get("type")
                                            .map_or("missing".into(), value_type_name),
                                        code: if block
                                            .as_object()
                                            .is_some_and(|o| o.contains_key("type"))
                                        {
                                            ParseErrorCode::InvalidType
                                        } else {
                                            ParseErrorCode::MissingField
                                        },
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // system must be string or array if present
        if let Some(sys) = obj.get("system") {
            if !sys.is_string() && !sys.is_array() {
                errors.push(ParseError {
                    field_path: "system".into(),
                    expected: "string or array".into(),
                    actual: value_type_name(sys),
                    code: ParseErrorCode::InvalidType,
                });
            }
        }

        // tools
        if let Some(Value::Array(tools)) = obj.get("tools") {
            for (i, tool) in tools.iter().enumerate() {
                let path = format!("tools[{i}]");
                if let Some(obj) = tool.as_object() {
                    if obj.get("name").and_then(Value::as_str).is_none() {
                        errors.push(ParseError {
                            field_path: format!("{path}.name"),
                            expected: "non-empty string".into(),
                            actual: obj.get("name").map_or("missing".into(), value_type_name),
                            code: if obj.contains_key("name") {
                                ParseErrorCode::InvalidType
                            } else {
                                ParseErrorCode::MissingField
                            },
                        });
                    }
                    if obj.get("input_schema").is_none() {
                        errors.push(ParseError {
                            field_path: format!("{path}.input_schema"),
                            expected: "object".into(),
                            actual: "missing".into(),
                            code: ParseErrorCode::MissingField,
                        });
                    }
                } else {
                    errors.push(ParseError {
                        field_path: path,
                        expected: "object".into(),
                        actual: value_type_name(tool),
                        code: ParseErrorCode::InvalidType,
                    });
                }
            }
        } else if let Some(v) = obj.get("tools") {
            errors.push(ParseError {
                field_path: "tools".into(),
                expected: "array".into(),
                actual: value_type_name(v),
                code: ParseErrorCode::InvalidType,
            });
        }

        ParseResult { errors }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Gemini dialect parser
// ═══════════════════════════════════════════════════════════════════════

const GEMINI_ROLES: &[&str] = &["user", "model"];

/// Strict parser for Google Gemini GenerateContent request schema.
#[derive(Debug, Default)]
pub struct GeminiParser;

impl DialectParser for GeminiParser {
    fn dialect(&self) -> Dialect {
        Dialect::Gemini
    }

    fn parse(&self, request: &Value) -> ParseResult {
        let mut errors = Vec::new();
        let Some(obj) = request.as_object() else {
            errors.push(ParseError {
                field_path: "<root>".into(),
                expected: "object".into(),
                actual: value_type_name(request),
                code: ParseErrorCode::InvalidType,
            });
            return ParseResult { errors };
        };

        check_model_present(obj, &mut errors);

        if let Some(contents) = require_array_field(obj, "contents", &mut errors) {
            if contents.is_empty() {
                errors.push(ParseError {
                    field_path: "contents".into(),
                    expected: "non-empty array".into(),
                    actual: "empty array".into(),
                    code: ParseErrorCode::EmptyArray,
                });
            } else {
                for (i, content) in contents.iter().enumerate() {
                    let prefix = format!("contents[{i}]");
                    let Some(cobj) = content.as_object() else {
                        errors.push(ParseError {
                            field_path: prefix,
                            expected: "object".into(),
                            actual: value_type_name(content),
                            code: ParseErrorCode::InvalidType,
                        });
                        continue;
                    };

                    // role is optional but must be valid if present
                    if let Some(role_val) = cobj.get("role") {
                        match role_val.as_str() {
                            Some(r) if GEMINI_ROLES.contains(&r) => {}
                            Some(r) => errors.push(ParseError {
                                field_path: format!("{prefix}.role"),
                                expected: format!("one of: {}", GEMINI_ROLES.join(", ")),
                                actual: format!("string: \"{r}\""),
                                code: ParseErrorCode::InvalidEnumValue,
                            }),
                            None => errors.push(ParseError {
                                field_path: format!("{prefix}.role"),
                                expected: "string".into(),
                                actual: value_type_name(role_val),
                                code: ParseErrorCode::InvalidType,
                            }),
                        }
                    }

                    // parts is required
                    match cobj.get("parts") {
                        None => errors.push(ParseError {
                            field_path: format!("{prefix}.parts"),
                            expected: "array".into(),
                            actual: "missing".into(),
                            code: ParseErrorCode::MissingField,
                        }),
                        Some(Value::Array(parts)) => {
                            if parts.is_empty() {
                                errors.push(ParseError {
                                    field_path: format!("{prefix}.parts"),
                                    expected: "non-empty array".into(),
                                    actual: "empty array".into(),
                                    code: ParseErrorCode::EmptyArray,
                                });
                            }
                        }
                        Some(other) => errors.push(ParseError {
                            field_path: format!("{prefix}.parts"),
                            expected: "array".into(),
                            actual: value_type_name(other),
                            code: ParseErrorCode::InvalidType,
                        }),
                    }
                }
            }
        }

        // generationConfig type check
        if let Some(gc) = obj.get("generationConfig") {
            if !gc.is_object() {
                errors.push(ParseError {
                    field_path: "generationConfig".into(),
                    expected: "object".into(),
                    actual: value_type_name(gc),
                    code: ParseErrorCode::InvalidType,
                });
            }
        }

        // tools
        if let Some(Value::Array(tools)) = obj.get("tools") {
            for (i, tool) in tools.iter().enumerate() {
                let path = format!("tools[{i}]");
                if let Some(tobj) = tool.as_object() {
                    if let Some(Value::Array(fds)) = tobj.get("functionDeclarations") {
                        for (j, fd) in fds.iter().enumerate() {
                            if fd.get("name").and_then(Value::as_str).is_none() {
                                errors.push(ParseError {
                                    field_path: format!("{path}.functionDeclarations[{j}].name"),
                                    expected: "non-empty string".into(),
                                    actual: fd
                                        .get("name")
                                        .map_or("missing".into(), value_type_name),
                                    code: ParseErrorCode::MissingField,
                                });
                            }
                        }
                    }
                } else {
                    errors.push(ParseError {
                        field_path: path,
                        expected: "object".into(),
                        actual: value_type_name(tool),
                        code: ParseErrorCode::InvalidType,
                    });
                }
            }
        } else if let Some(v) = obj.get("tools") {
            errors.push(ParseError {
                field_path: "tools".into(),
                expected: "array".into(),
                actual: value_type_name(v),
                code: ParseErrorCode::InvalidType,
            });
        }

        ParseResult { errors }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Codex dialect parser
// ═══════════════════════════════════════════════════════════════════════

const CODEX_ROLES: &[&str] = &["user", "assistant", "system", "developer"];

/// Strict parser for OpenAI Codex / Responses API request schema.
#[derive(Debug, Default)]
pub struct CodexParser;

impl DialectParser for CodexParser {
    fn dialect(&self) -> Dialect {
        Dialect::Codex
    }

    fn parse(&self, request: &Value) -> ParseResult {
        let mut errors = Vec::new();
        let Some(obj) = request.as_object() else {
            errors.push(ParseError {
                field_path: "<root>".into(),
                expected: "object".into(),
                actual: value_type_name(request),
                code: ParseErrorCode::InvalidType,
            });
            return ParseResult { errors };
        };

        check_model_present(obj, &mut errors);
        check_stream_field(obj, &mut errors);

        // Codex Responses API uses "input" which can be a string or array of messages
        match obj.get("input") {
            None => errors.push(ParseError {
                field_path: "input".into(),
                expected: "string or array".into(),
                actual: "missing".into(),
                code: ParseErrorCode::MissingField,
            }),
            Some(Value::String(_)) => {}
            Some(Value::Array(msgs)) => {
                if msgs.is_empty() {
                    errors.push(ParseError {
                        field_path: "input".into(),
                        expected: "non-empty array".into(),
                        actual: "empty array".into(),
                        code: ParseErrorCode::EmptyArray,
                    });
                } else {
                    check_roles(msgs, "input", CODEX_ROLES, &mut errors);
                }
            }
            Some(other) => errors.push(ParseError {
                field_path: "input".into(),
                expected: "string or array".into(),
                actual: value_type_name(other),
                code: ParseErrorCode::InvalidType,
            }),
        }

        // instructions (optional string)
        if let Some(v) = obj.get("instructions") {
            if !v.is_string() {
                errors.push(ParseError {
                    field_path: "instructions".into(),
                    expected: "string".into(),
                    actual: value_type_name(v),
                    code: ParseErrorCode::InvalidType,
                });
            }
        }

        // tools
        if let Some(Value::Array(tools)) = obj.get("tools") {
            validate_tool_definitions(tools, "tools", &mut errors);
        } else if let Some(v) = obj.get("tools") {
            errors.push(ParseError {
                field_path: "tools".into(),
                expected: "array".into(),
                actual: value_type_name(v),
                code: ParseErrorCode::InvalidType,
            });
        }

        ParseResult { errors }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Copilot dialect parser
// ═══════════════════════════════════════════════════════════════════════

const COPILOT_ROLES: &[&str] = &["system", "user", "assistant", "function"];

/// Strict parser for GitHub Copilot chat request schema.
#[derive(Debug, Default)]
pub struct CopilotParser;

impl DialectParser for CopilotParser {
    fn dialect(&self) -> Dialect {
        Dialect::Copilot
    }

    fn parse(&self, request: &Value) -> ParseResult {
        let mut errors = Vec::new();
        let Some(obj) = request.as_object() else {
            errors.push(ParseError {
                field_path: "<root>".into(),
                expected: "object".into(),
                actual: value_type_name(request),
                code: ParseErrorCode::InvalidType,
            });
            return ParseResult { errors };
        };

        check_model_present(obj, &mut errors);
        check_stream_field(obj, &mut errors);

        if let Some(msgs) = require_array_field(obj, "messages", &mut errors) {
            if msgs.is_empty() {
                errors.push(ParseError {
                    field_path: "messages".into(),
                    expected: "non-empty array".into(),
                    actual: "empty array".into(),
                    code: ParseErrorCode::EmptyArray,
                });
            } else {
                check_roles(&msgs, "messages", COPILOT_ROLES, &mut errors);
            }
        }

        // references must be array if present
        if let Some(v) = obj.get("references") {
            if !v.is_array() {
                errors.push(ParseError {
                    field_path: "references".into(),
                    expected: "array".into(),
                    actual: value_type_name(v),
                    code: ParseErrorCode::InvalidType,
                });
            }
        }

        // tools
        if let Some(Value::Array(tools)) = obj.get("tools") {
            validate_tool_definitions(tools, "tools", &mut errors);
        } else if let Some(v) = obj.get("tools") {
            errors.push(ParseError {
                field_path: "tools".into(),
                expected: "array".into(),
                actual: value_type_name(v),
                code: ParseErrorCode::InvalidType,
            });
        }

        ParseResult { errors }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Kimi dialect parser
// ═══════════════════════════════════════════════════════════════════════

const KIMI_ROLES: &[&str] = &["system", "user", "assistant", "tool"];

/// Strict parser for Moonshot Kimi request schema.
#[derive(Debug, Default)]
pub struct KimiParser;

impl DialectParser for KimiParser {
    fn dialect(&self) -> Dialect {
        Dialect::Kimi
    }

    fn parse(&self, request: &Value) -> ParseResult {
        let mut errors = Vec::new();
        let Some(obj) = request.as_object() else {
            errors.push(ParseError {
                field_path: "<root>".into(),
                expected: "object".into(),
                actual: value_type_name(request),
                code: ParseErrorCode::InvalidType,
            });
            return ParseResult { errors };
        };

        check_model_present(obj, &mut errors);
        check_stream_field(obj, &mut errors);

        if let Some(msgs) = require_array_field(obj, "messages", &mut errors) {
            if msgs.is_empty() {
                errors.push(ParseError {
                    field_path: "messages".into(),
                    expected: "non-empty array".into(),
                    actual: "empty array".into(),
                    code: ParseErrorCode::EmptyArray,
                });
            } else {
                check_roles(&msgs, "messages", KIMI_ROLES, &mut errors);
            }
        }

        // search_plus must be boolean if present
        if let Some(v) = obj.get("search_plus") {
            if !v.is_boolean() {
                errors.push(ParseError {
                    field_path: "search_plus".into(),
                    expected: "boolean".into(),
                    actual: value_type_name(v),
                    code: ParseErrorCode::InvalidType,
                });
            }
        }

        // tools
        if let Some(Value::Array(tools)) = obj.get("tools") {
            validate_tool_definitions(tools, "tools", &mut errors);
        } else if let Some(v) = obj.get("tools") {
            errors.push(ParseError {
                field_path: "tools".into(),
                expected: "array".into(),
                actual: value_type_name(v),
                code: ParseErrorCode::InvalidType,
            });
        }

        // temperature must be a number if present
        if let Some(v) = obj.get("temperature") {
            if !v.is_number() {
                errors.push(ParseError {
                    field_path: "temperature".into(),
                    expected: "number".into(),
                    actual: value_type_name(v),
                    code: ParseErrorCode::InvalidType,
                });
            }
        }

        ParseResult { errors }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Dispatch helper
// ═══════════════════════════════════════════════════════════════════════

/// Returns the appropriate [`DialectParser`] for the given [`Dialect`].
#[must_use]
pub fn parser_for(dialect: Dialect) -> Box<dyn DialectParser> {
    match dialect {
        Dialect::OpenAi => Box::new(OpenAiParser),
        Dialect::Claude => Box::new(ClaudeParser),
        Dialect::Gemini => Box::new(GeminiParser),
        Dialect::Codex => Box::new(CodexParser),
        Dialect::Copilot => Box::new(CopilotParser),
        Dialect::Kimi => Box::new(KimiParser),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── ParseError display ──────────────────────────────────────────

    #[test]
    fn parse_error_display() {
        let e = ParseError {
            field_path: "model".into(),
            expected: "string".into(),
            actual: "number: 42".into(),
            code: ParseErrorCode::InvalidType,
        };
        let s = format!("{e}");
        assert!(s.contains("model"));
        assert!(s.contains("string"));
        assert!(s.contains("number: 42"));
        assert!(s.contains("invalid_type"));
    }

    #[test]
    fn parse_error_code_display() {
        assert_eq!(ParseErrorCode::MissingField.to_string(), "missing_field");
        assert_eq!(ParseErrorCode::InvalidType.to_string(), "invalid_type");
        assert_eq!(
            ParseErrorCode::InvalidEnumValue.to_string(),
            "invalid_enum_value"
        );
        assert_eq!(ParseErrorCode::EmptyArray.to_string(), "empty_array");
        assert_eq!(
            ParseErrorCode::InvalidStructure.to_string(),
            "invalid_structure"
        );
    }

    #[test]
    fn parse_result_is_ok_when_empty() {
        let r = ParseResult { errors: vec![] };
        assert!(r.is_ok());
    }

    // ── parser_for dispatch ─────────────────────────────────────────

    #[test]
    fn parser_for_all_dialects() {
        for &d in Dialect::all() {
            let p = parser_for(d);
            assert_eq!(p.dialect(), d);
        }
    }

    // ── OpenAI parser ───────────────────────────────────────────────

    #[test]
    fn openai_valid_minimal() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn openai_missing_model() {
        let r = OpenAiParser.parse(&json!({
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path == "model" && e.code == ParseErrorCode::MissingField)
        );
    }

    #[test]
    fn openai_missing_messages() {
        let r = OpenAiParser.parse(&json!({"model": "gpt-4"}));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "messages"));
    }

    #[test]
    fn openai_empty_messages() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": []
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == ParseErrorCode::EmptyArray)
        );
    }

    #[test]
    fn openai_invalid_role() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"role": "narrator", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == ParseErrorCode::InvalidEnumValue)
        );
    }

    #[test]
    fn openai_missing_role_in_message() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "messages[0].role"));
    }

    #[test]
    fn openai_valid_with_stream_true() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn openai_invalid_stream_type() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": "yes"
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "stream"));
    }

    #[test]
    fn openai_valid_tools() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {"name": "get_weather"}}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn openai_tool_missing_function_name() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {}}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path.contains("function.name"))
        );
    }

    #[test]
    fn openai_tool_missing_function_object() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function"}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path.contains("function")));
    }

    #[test]
    fn openai_invalid_temperature_type() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": "hot"
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "temperature"));
    }

    #[test]
    fn openai_non_object_root() {
        let r = OpenAiParser.parse(&json!(42));
        assert!(!r.is_ok());
        assert!(r.errors[0].field_path == "<root>");
    }

    #[test]
    fn openai_model_not_string() {
        let r = OpenAiParser.parse(&json!({
            "model": 42,
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path == "model" && e.code == ParseErrorCode::InvalidType)
        );
    }

    #[test]
    fn openai_empty_model_string() {
        let r = OpenAiParser.parse(&json!({
            "model": "",
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "model"));
    }

    #[test]
    fn openai_messages_not_array() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": "not an array"
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path == "messages" && e.code == ParseErrorCode::InvalidType)
        );
    }

    #[test]
    fn openai_tools_not_array() {
        let r = OpenAiParser.parse(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": "bad"
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "tools"));
    }

    #[test]
    fn openai_all_valid_roles() {
        for role in OPENAI_ROLES {
            let r = OpenAiParser.parse(&json!({
                "model": "gpt-4",
                "messages": [{"role": role, "content": "hi"}]
            }));
            assert!(r.is_ok(), "role {role} should be valid: {:?}", r.errors);
        }
    }

    // ── Claude parser ───────────────────────────────────────────────

    #[test]
    fn claude_valid_minimal() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn claude_missing_max_tokens() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "max_tokens"));
    }

    #[test]
    fn claude_max_tokens_not_number() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": "lots",
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path == "max_tokens" && e.code == ParseErrorCode::InvalidType)
        );
    }

    #[test]
    fn claude_invalid_role() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "system", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == ParseErrorCode::InvalidEnumValue)
        );
    }

    #[test]
    fn claude_content_blocks_valid() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn claude_content_block_missing_type() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": [{"text": "hi"}]}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path.contains("content[0].type"))
        );
    }

    #[test]
    fn claude_content_invalid_type() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": 42}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path.contains("content")));
    }

    #[test]
    fn claude_system_string_valid() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}],
            "system": "Be helpful"
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn claude_system_invalid_type() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}],
            "system": 42
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "system"));
    }

    #[test]
    fn claude_tools_valid() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"name": "search", "input_schema": {"type": "object"}}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn claude_tool_missing_name() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"input_schema": {"type": "object"}}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path.contains("name")));
    }

    #[test]
    fn claude_tool_missing_input_schema() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"name": "search"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path.contains("input_schema"))
        );
    }

    #[test]
    fn claude_stream_valid() {
        let r = ClaudeParser.parse(&json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "hi"}],
            "stream": false
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn claude_non_object_root() {
        let r = ClaudeParser.parse(&json!("hello"));
        assert!(!r.is_ok());
        assert!(r.errors[0].field_path == "<root>");
    }

    // ── Gemini parser ───────────────────────────────────────────────

    #[test]
    fn gemini_valid_minimal() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-1.5-pro",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn gemini_missing_model() {
        let r = GeminiParser.parse(&json!({
            "contents": [{"parts": [{"text": "hi"}]}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "model"));
    }

    #[test]
    fn gemini_missing_contents() {
        let r = GeminiParser.parse(&json!({"model": "gemini-pro"}));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "contents"));
    }

    #[test]
    fn gemini_empty_contents() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": []
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == ParseErrorCode::EmptyArray)
        );
    }

    #[test]
    fn gemini_missing_parts() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": [{"role": "user"}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "contents[0].parts"));
    }

    #[test]
    fn gemini_empty_parts() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": [{"parts": []}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors.iter().any(
                |e| e.field_path == "contents[0].parts" && e.code == ParseErrorCode::EmptyArray
            )
        );
    }

    #[test]
    fn gemini_invalid_role() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": [{"role": "system", "parts": [{"text": "hi"}]}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == ParseErrorCode::InvalidEnumValue)
        );
    }

    #[test]
    fn gemini_valid_generation_config() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": [{"parts": [{"text": "hi"}]}],
            "generationConfig": {"temperature": 0.7}
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn gemini_generation_config_not_object() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": [{"parts": [{"text": "hi"}]}],
            "generationConfig": "bad"
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "generationConfig"));
    }

    #[test]
    fn gemini_tools_valid() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": [{"parts": [{"text": "hi"}]}],
            "tools": [{"functionDeclarations": [{"name": "search"}]}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn gemini_tool_declaration_missing_name() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": [{"parts": [{"text": "hi"}]}],
            "tools": [{"functionDeclarations": [{"description": "no name"}]}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path.contains("functionDeclarations"))
        );
    }

    #[test]
    fn gemini_contents_entry_not_object() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": ["not an object"]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "contents[0]"));
    }

    #[test]
    fn gemini_parts_not_array() {
        let r = GeminiParser.parse(&json!({
            "model": "gemini-pro",
            "contents": [{"parts": "bad"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path == "contents[0].parts"
                    && e.code == ParseErrorCode::InvalidType)
        );
    }

    #[test]
    fn gemini_non_object_root() {
        let r = GeminiParser.parse(&json!([]));
        assert!(!r.is_ok());
        assert!(r.errors[0].field_path == "<root>");
    }

    // ── Codex parser ────────────────────────────────────────────────

    #[test]
    fn codex_valid_string_input() {
        let r = CodexParser.parse(&json!({
            "model": "codex-mini",
            "input": "fix the bug"
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn codex_valid_array_input() {
        let r = CodexParser.parse(&json!({
            "model": "codex-mini",
            "input": [{"role": "user", "content": "fix bug"}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn codex_missing_model() {
        let r = CodexParser.parse(&json!({
            "input": "fix"
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "model"));
    }

    #[test]
    fn codex_missing_input() {
        let r = CodexParser.parse(&json!({
            "model": "codex-mini"
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "input"));
    }

    #[test]
    fn codex_empty_input_array() {
        let r = CodexParser.parse(&json!({
            "model": "codex-mini",
            "input": []
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == ParseErrorCode::EmptyArray)
        );
    }

    #[test]
    fn codex_input_invalid_type() {
        let r = CodexParser.parse(&json!({
            "model": "codex-mini",
            "input": 42
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.field_path == "input" && e.code == ParseErrorCode::InvalidType)
        );
    }

    #[test]
    fn codex_invalid_role_in_input() {
        let r = CodexParser.parse(&json!({
            "model": "codex-mini",
            "input": [{"role": "narrator", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == ParseErrorCode::InvalidEnumValue)
        );
    }

    #[test]
    fn codex_instructions_not_string() {
        let r = CodexParser.parse(&json!({
            "model": "codex-mini",
            "input": "fix",
            "instructions": 42
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "instructions"));
    }

    #[test]
    fn codex_stream_valid() {
        let r = CodexParser.parse(&json!({
            "model": "codex-mini",
            "input": "fix",
            "stream": true
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn codex_tools_valid() {
        let r = CodexParser.parse(&json!({
            "model": "codex-mini",
            "input": "fix",
            "tools": [{"type": "function", "function": {"name": "run_test"}}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    // ── Copilot parser ──────────────────────────────────────────────

    #[test]
    fn copilot_valid_minimal() {
        let r = CopilotParser.parse(&json!({
            "model": "copilot-chat",
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn copilot_missing_model() {
        let r = CopilotParser.parse(&json!({
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "model"));
    }

    #[test]
    fn copilot_missing_messages() {
        let r = CopilotParser.parse(&json!({"model": "copilot-chat"}));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "messages"));
    }

    #[test]
    fn copilot_invalid_role() {
        let r = CopilotParser.parse(&json!({
            "model": "copilot-chat",
            "messages": [{"role": "tool", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == ParseErrorCode::InvalidEnumValue)
        );
    }

    #[test]
    fn copilot_references_not_array() {
        let r = CopilotParser.parse(&json!({
            "model": "copilot-chat",
            "messages": [{"role": "user", "content": "hi"}],
            "references": "bad"
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "references"));
    }

    #[test]
    fn copilot_references_valid() {
        let r = CopilotParser.parse(&json!({
            "model": "copilot-chat",
            "messages": [{"role": "user", "content": "hi"}],
            "references": [{"type": "file", "path": "src/main.rs"}]
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn copilot_stream_valid() {
        let r = CopilotParser.parse(&json!({
            "model": "copilot-chat",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn copilot_all_valid_roles() {
        for role in COPILOT_ROLES {
            let r = CopilotParser.parse(&json!({
                "model": "copilot-chat",
                "messages": [{"role": role, "content": "hi"}]
            }));
            assert!(r.is_ok(), "role {role} should be valid: {:?}", r.errors);
        }
    }

    #[test]
    fn copilot_non_object_root() {
        let r = CopilotParser.parse(&json!(null));
        assert!(!r.is_ok());
        assert!(r.errors[0].field_path == "<root>");
    }

    // ── Kimi parser ─────────────────────────────────────────────────

    #[test]
    fn kimi_valid_minimal() {
        let r = KimiParser.parse(&json!({
            "model": "moonshot-v1-32k",
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn kimi_missing_model() {
        let r = KimiParser.parse(&json!({
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "model"));
    }

    #[test]
    fn kimi_missing_messages() {
        let r = KimiParser.parse(&json!({"model": "moonshot-v1"}));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "messages"));
    }

    #[test]
    fn kimi_invalid_role() {
        let r = KimiParser.parse(&json!({
            "model": "moonshot-v1",
            "messages": [{"role": "function", "content": "hi"}]
        }));
        assert!(!r.is_ok());
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == ParseErrorCode::InvalidEnumValue)
        );
    }

    #[test]
    fn kimi_search_plus_not_bool() {
        let r = KimiParser.parse(&json!({
            "model": "moonshot-v1",
            "messages": [{"role": "user", "content": "hi"}],
            "search_plus": "true"
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "search_plus"));
    }

    #[test]
    fn kimi_search_plus_valid() {
        let r = KimiParser.parse(&json!({
            "model": "moonshot-v1",
            "messages": [{"role": "user", "content": "hi"}],
            "search_plus": true
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn kimi_temperature_not_number() {
        let r = KimiParser.parse(&json!({
            "model": "moonshot-v1",
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": "warm"
        }));
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.field_path == "temperature"));
    }

    #[test]
    fn kimi_all_valid_roles() {
        for role in KIMI_ROLES {
            let r = KimiParser.parse(&json!({
                "model": "moonshot-v1",
                "messages": [{"role": role, "content": "hi"}]
            }));
            assert!(r.is_ok(), "role {role} should be valid: {:?}", r.errors);
        }
    }

    #[test]
    fn kimi_tools_valid() {
        let r = KimiParser.parse(&json!({
            "model": "moonshot-v1",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {"name": "search"}}]
        }));
        assert!(r.is_ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn kimi_non_object_root() {
        let r = KimiParser.parse(&json!(true));
        assert!(!r.is_ok());
    }

    // ── Cross-cutting ───────────────────────────────────────────────

    #[test]
    fn all_parsers_reject_non_object() {
        for &d in Dialect::all() {
            let p = parser_for(d);
            let r = p.parse(&json!(42));
            assert!(!r.is_ok(), "{d:?} should reject non-object");
        }
    }

    #[test]
    fn all_parsers_reject_empty_object() {
        for &d in Dialect::all() {
            let p = parser_for(d);
            let r = p.parse(&json!({}));
            assert!(!r.is_ok(), "{d:?} should reject empty object");
        }
    }

    #[test]
    fn multiple_errors_accumulated() {
        // Missing model AND messages
        let r = OpenAiParser.parse(&json!({}));
        assert!(r.errors.len() >= 2);
    }

    #[test]
    fn value_type_name_covers_all_variants() {
        assert!(value_type_name(&json!(null)).contains("null"));
        assert!(value_type_name(&json!(true)).contains("bool"));
        assert!(value_type_name(&json!(42)).contains("number"));
        assert!(value_type_name(&json!("hi")).contains("string"));
        assert!(value_type_name(&json!([])).contains("array"));
        assert!(value_type_name(&json!({})).contains("object"));
    }

    #[test]
    fn value_type_name_truncates_long_strings() {
        let long = "a".repeat(100);
        let name = value_type_name(&Value::String(long));
        assert!(name.contains("..."));
        assert!(name.len() < 60);
    }
}
