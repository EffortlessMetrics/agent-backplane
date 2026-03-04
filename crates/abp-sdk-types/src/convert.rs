// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-dialect type conversion utilities.
//!
//! Provides a [`DialectConverter`] trait for translating canonical messages,
//! tools, and responses between vendor dialects, along with a [`RoleMapper`]
//! helper for role-name mapping and a [`ConversionReport`] for tracking
//! conversion outcomes.

use crate::Dialect;
use serde::{Deserialize, Serialize};

// ── Canonical message for conversion ────────────────────────────────────

/// A dialect-agnostic message used as the canonical form during conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct Message {
    /// Message role (e.g. `"system"`, `"user"`, `"assistant"`, `"tool"`).
    pub role: String,
    /// Text content of the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Optional tool call identifier this message responds to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

// ── Canonical tool definition for conversion ────────────────────────────

/// A dialect-agnostic tool definition used during cross-dialect conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    pub parameters: serde_json::Value,
}

// ── ConversionError ─────────────────────────────────────────────────────

/// Errors that can occur during cross-dialect type conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversionError {
    /// A field is not supported in the target dialect.
    UnsupportedField {
        /// Name of the unsupported field.
        field: String,
        /// The target dialect that does not support this field.
        dialect: Dialect,
    },
    /// A type mismatch between source and target representations.
    IncompatibleType {
        /// The source type description.
        source_type: String,
        /// The target type description.
        target_type: String,
    },
    /// A field required in the target dialect is absent in the source.
    MissingRequiredField {
        /// Name of the missing field.
        field: String,
    },
    /// Content exceeds the target dialect's size limit.
    ContentTooLong {
        /// Maximum allowed length.
        max: usize,
        /// Actual content length.
        actual: usize,
    },
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedField { field, dialect } => {
                write!(f, "field `{field}` not supported in {dialect}")
            }
            Self::IncompatibleType {
                source_type,
                target_type,
            } => write!(f, "cannot convert `{source_type}` to `{target_type}`"),
            Self::MissingRequiredField { field } => {
                write!(f, "required field `{field}` is missing")
            }
            Self::ContentTooLong { max, actual } => {
                write!(f, "content length {actual} exceeds maximum {max}")
            }
        }
    }
}

impl std::error::Error for ConversionError {}

// ── ConversionReport ────────────────────────────────────────────────────

/// Summary of a cross-dialect conversion operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ConversionReport {
    /// Source dialect of the conversion.
    pub source: Dialect,
    /// Target dialect of the conversion.
    pub target: Dialect,
    /// Number of successful conversions performed.
    pub conversions: usize,
    /// Non-fatal warnings encountered during conversion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Fatal errors encountered during conversion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ConversionError>,
    /// Whether the conversion was lossless (no information lost).
    pub is_lossless: bool,
}

impl ConversionReport {
    /// Creates a new empty report for the given dialect pair.
    #[must_use]
    pub fn new(source: Dialect, target: Dialect) -> Self {
        Self {
            source,
            target,
            conversions: 0,
            warnings: Vec::new(),
            errors: Vec::new(),
            is_lossless: true,
        }
    }

    /// Returns `true` if no fatal errors were recorded.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

// ── DialectConverter trait ───────────────────────────────────────────────

/// Trait for converting messages, tools, and responses between dialects.
///
/// Implementors define how to translate canonical [`Message`] and
/// [`ToolDefinition`] values from one dialect to another, as well as
/// raw JSON responses.
pub trait DialectConverter {
    /// The source dialect this converter reads from.
    fn source_dialect(&self) -> Dialect;
    /// The target dialect this converter writes to.
    fn target_dialect(&self) -> Dialect;
    /// Convert a canonical message to the target dialect's representation.
    fn convert_message(&self, msg: &Message) -> Result<Message, ConversionError>;
    /// Convert a canonical tool definition to the target dialect's representation.
    fn convert_tool(&self, tool: &ToolDefinition) -> Result<ToolDefinition, ConversionError>;
    /// Convert a raw JSON response to the target dialect's representation.
    fn convert_response(
        &self,
        resp: &serde_json::Value,
    ) -> Result<serde_json::Value, ConversionError>;
}

// ── RoleMapper ──────────────────────────────────────────────────────────

/// Helper for mapping role names between dialects.
///
/// Each dialect uses different role strings:
/// - **OpenAI / Codex / Copilot / Kimi**: `"system"`, `"user"`, `"assistant"`, `"tool"`
/// - **Claude**: `"user"`, `"assistant"` (system goes to a separate `system` field)
/// - **Gemini**: `"user"`, `"model"`
pub struct RoleMapper;

impl RoleMapper {
    /// Maps a role string from one dialect to another.
    ///
    /// # Errors
    ///
    /// Returns [`ConversionError::UnsupportedField`] if the role has no
    /// equivalent in the target dialect (e.g. `"system"` → Claude messages).
    /// Returns [`ConversionError::IncompatibleType`] if the role is unknown
    /// in the source dialect.
    pub fn map_role(role: &str, from: Dialect, to: Dialect) -> Result<String, ConversionError> {
        // Normalize to a canonical internal key.
        let canonical = Self::to_canonical(role, from)?;
        Self::from_canonical(&canonical, to)
    }

    /// Normalizes a dialect-specific role string to a canonical key.
    fn to_canonical(role: &str, dialect: Dialect) -> Result<CanonicalRole, ConversionError> {
        match dialect {
            Dialect::OpenAi | Dialect::Codex | Dialect::Copilot | Dialect::Kimi => match role {
                "system" => Ok(CanonicalRole::System),
                "user" => Ok(CanonicalRole::User),
                "assistant" => Ok(CanonicalRole::Assistant),
                "tool" => Ok(CanonicalRole::Tool),
                other => Err(ConversionError::IncompatibleType {
                    source_type: format!("role `{other}`"),
                    target_type: format!("{dialect} role"),
                }),
            },
            Dialect::Claude => match role {
                "user" => Ok(CanonicalRole::User),
                "assistant" => Ok(CanonicalRole::Assistant),
                other => Err(ConversionError::IncompatibleType {
                    source_type: format!("role `{other}`"),
                    target_type: format!("{dialect} role"),
                }),
            },
            Dialect::Gemini => match role {
                "user" => Ok(CanonicalRole::User),
                "model" => Ok(CanonicalRole::Assistant),
                other => Err(ConversionError::IncompatibleType {
                    source_type: format!("role `{other}`"),
                    target_type: format!("{dialect} role"),
                }),
            },
        }
    }

    /// Converts a canonical role to the target dialect's string.
    fn from_canonical(
        canonical: &CanonicalRole,
        dialect: Dialect,
    ) -> Result<String, ConversionError> {
        match dialect {
            Dialect::OpenAi | Dialect::Codex | Dialect::Copilot | Dialect::Kimi => {
                match canonical {
                    CanonicalRole::System => Ok("system".into()),
                    CanonicalRole::User => Ok("user".into()),
                    CanonicalRole::Assistant => Ok("assistant".into()),
                    CanonicalRole::Tool => Ok("tool".into()),
                }
            }
            Dialect::Claude => match canonical {
                CanonicalRole::System => Err(ConversionError::UnsupportedField {
                    field: "system".into(),
                    dialect,
                }),
                CanonicalRole::User => Ok("user".into()),
                CanonicalRole::Assistant => Ok("assistant".into()),
                CanonicalRole::Tool => Err(ConversionError::UnsupportedField {
                    field: "tool".into(),
                    dialect,
                }),
            },
            Dialect::Gemini => match canonical {
                CanonicalRole::System => Err(ConversionError::UnsupportedField {
                    field: "system".into(),
                    dialect,
                }),
                CanonicalRole::User => Ok("user".into()),
                CanonicalRole::Assistant => Ok("model".into()),
                CanonicalRole::Tool => Err(ConversionError::UnsupportedField {
                    field: "tool".into(),
                    dialect,
                }),
            },
        }
    }
}

/// Internal canonical role used during dialect mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanonicalRole {
    System,
    User,
    Assistant,
    Tool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_error_display_unsupported() {
        let err = ConversionError::UnsupportedField {
            field: "system".into(),
            dialect: Dialect::Claude,
        };
        assert!(err.to_string().contains("system"));
        assert!(err.to_string().contains("Claude"));
    }

    #[test]
    fn conversion_error_display_incompatible() {
        let err = ConversionError::IncompatibleType {
            source_type: "string".into(),
            target_type: "array".into(),
        };
        assert!(err.to_string().contains("string"));
        assert!(err.to_string().contains("array"));
    }

    #[test]
    fn conversion_error_display_missing() {
        let err = ConversionError::MissingRequiredField {
            field: "content".into(),
        };
        assert!(err.to_string().contains("content"));
    }

    #[test]
    fn conversion_error_display_too_long() {
        let err = ConversionError::ContentTooLong {
            max: 100,
            actual: 200,
        };
        let s = err.to_string();
        assert!(s.contains("100"));
        assert!(s.contains("200"));
    }

    #[test]
    fn report_new_defaults() {
        let r = ConversionReport::new(Dialect::OpenAi, Dialect::Claude);
        assert_eq!(r.source, Dialect::OpenAi);
        assert_eq!(r.target, Dialect::Claude);
        assert_eq!(r.conversions, 0);
        assert!(r.warnings.is_empty());
        assert!(r.errors.is_empty());
        assert!(r.is_lossless);
        assert!(r.is_ok());
    }

    #[test]
    fn report_is_ok_with_errors() {
        let mut r = ConversionReport::new(Dialect::OpenAi, Dialect::Gemini);
        r.errors.push(ConversionError::MissingRequiredField {
            field: "role".into(),
        });
        assert!(!r.is_ok());
    }

    #[test]
    fn role_mapper_openai_to_openai() {
        assert_eq!(
            RoleMapper::map_role("user", Dialect::OpenAi, Dialect::OpenAi).unwrap(),
            "user"
        );
        assert_eq!(
            RoleMapper::map_role("system", Dialect::OpenAi, Dialect::OpenAi).unwrap(),
            "system"
        );
        assert_eq!(
            RoleMapper::map_role("assistant", Dialect::OpenAi, Dialect::OpenAi).unwrap(),
            "assistant"
        );
        assert_eq!(
            RoleMapper::map_role("tool", Dialect::OpenAi, Dialect::OpenAi).unwrap(),
            "tool"
        );
    }

    #[test]
    fn role_mapper_openai_to_gemini() {
        assert_eq!(
            RoleMapper::map_role("user", Dialect::OpenAi, Dialect::Gemini).unwrap(),
            "user"
        );
        assert_eq!(
            RoleMapper::map_role("assistant", Dialect::OpenAi, Dialect::Gemini).unwrap(),
            "model"
        );
    }

    #[test]
    fn role_mapper_gemini_to_openai() {
        assert_eq!(
            RoleMapper::map_role("model", Dialect::Gemini, Dialect::OpenAi).unwrap(),
            "assistant"
        );
    }

    #[test]
    fn role_mapper_system_to_claude_fails() {
        let err = RoleMapper::map_role("system", Dialect::OpenAi, Dialect::Claude).unwrap_err();
        assert!(matches!(err, ConversionError::UnsupportedField { .. }));
    }

    #[test]
    fn role_mapper_system_to_gemini_fails() {
        let err = RoleMapper::map_role("system", Dialect::OpenAi, Dialect::Gemini).unwrap_err();
        assert!(matches!(err, ConversionError::UnsupportedField { .. }));
    }

    #[test]
    fn role_mapper_unknown_role_fails() {
        let err = RoleMapper::map_role("narrator", Dialect::OpenAi, Dialect::Claude).unwrap_err();
        assert!(matches!(err, ConversionError::IncompatibleType { .. }));
    }
}
