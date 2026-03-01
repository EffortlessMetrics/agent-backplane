// SPDX-License-Identifier: MIT OR Apache-2.0
//! Structured output `response_format` types for the OpenAI Chat Completions API.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ResponseFormat enum
// ---------------------------------------------------------------------------

/// The `response_format` parameter for Chat Completions requests.
///
/// Controls the output format of the model response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    /// Plain text output (default behavior).
    Text,
    /// JSON object output (guarantees valid JSON).
    JsonObject,
    /// JSON output conforming to a specific JSON Schema.
    JsonSchema {
        /// The JSON Schema specification.
        json_schema: JsonSchemaSpec,
    },
}

/// A JSON Schema specification for structured output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonSchemaSpec {
    /// Human-readable name for this schema.
    pub name: String,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The JSON Schema itself.
    pub schema: serde_json::Value,
    /// Whether to enforce strict schema adherence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

impl ResponseFormat {
    /// Create a plain text response format.
    #[must_use]
    pub fn text() -> Self {
        Self::Text
    }

    /// Create a JSON object response format.
    #[must_use]
    pub fn json_object() -> Self {
        Self::JsonObject
    }

    /// Create a JSON Schema response format.
    #[must_use]
    pub fn json_schema(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self::JsonSchema {
            json_schema: JsonSchemaSpec {
                name: name.into(),
                description: None,
                schema,
                strict: Some(true),
            },
        }
    }
}
