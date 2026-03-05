// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extended Claude tool-use types.
//!
//! Supplements the core [`ToolDefinition`](crate::claude_types::ToolDefinition)
//! and [`ToolChoice`](crate::claude_types::ToolChoice) with richer schema
//! modelling, image-bearing tool results, and cache-control annotations.

use serde::{Deserialize, Serialize};

use crate::claude_types::{CacheControl, ImageSource};

// ── Input schema ────────────────────────────────────────────────────────

/// Strongly-typed JSON-Schema wrapper for tool input parameters.
///
/// While [`ToolDefinition::input_schema`](crate::claude_types::ToolDefinition)
/// uses an opaque `serde_json::Value`, this struct provides typed access
/// to the most common fields so callers can construct schemas in Rust
/// rather than hand-rolling JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSchema {
    /// Must be `"object"`.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Property definitions keyed by parameter name.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub properties: serde_json::Map<String, serde_json::Value>,
    /// Names of required parameters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
}

impl InputSchema {
    /// Create an empty object schema.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_type: "object".into(),
            properties: serde_json::Map::new(),
            required: Vec::new(),
        }
    }

    /// Add a property to the schema.
    #[must_use]
    pub fn with_property(
        mut self,
        name: impl Into<String>,
        schema: serde_json::Value,
        required: bool,
    ) -> Self {
        let name = name.into();
        if required {
            self.required.push(name.clone());
        }
        self.properties.insert(name, schema);
        self
    }

    /// Convert to an opaque `serde_json::Value` for embedding in a
    /// [`ToolDefinition`](crate::claude_types::ToolDefinition).
    #[must_use]
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

// ── Tool definition with cache control ──────────────────────────────────

/// Extended tool definition that supports prompt-caching annotations.
///
/// Claude's API allows a `cache_control` key on each tool definition so
/// that expensive tool schemas can be cached across turns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedToolDefinition {
    /// Tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for input parameters.
    pub input_schema: serde_json::Value,
    /// Optional prompt-caching directive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl CachedToolDefinition {
    /// Create a new cached tool definition with ephemeral cache control.
    #[must_use]
    pub fn ephemeral(
        name: impl Into<String>,
        description: impl Into<String>,
        schema: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: schema,
            cache_control: Some(CacheControl {
                cache_type: "ephemeral".into(),
            }),
        }
    }
}

// ── Rich tool result content ────────────────────────────────────────────

/// A content part within a tool result.
///
/// Claude tool results can contain text and/or images (e.g. screenshots).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContent {
    /// Plain text result.
    Text {
        /// The text payload.
        text: String,
    },
    /// Image result (e.g. a screenshot captured by a tool).
    Image {
        /// Image source.
        source: ImageSource,
    },
}

/// A rich tool result block that can carry multiple content parts.
///
/// This is the structured variant of
/// [`ContentBlock::ToolResult`](crate::claude_types::ContentBlock::ToolResult)
/// where `content` is a list of [`ToolResultContent`] items rather than
/// a single optional string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RichToolResult {
    /// Identifier of the corresponding tool-use block.
    pub tool_use_id: String,
    /// Content parts making up the result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ToolResultContent>,
    /// Whether the tool reported an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl RichToolResult {
    /// Create a text-only tool result.
    #[must_use]
    pub fn text(tool_use_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: vec![ToolResultContent::Text { text: text.into() }],
            is_error: None,
        }
    }

    /// Create an error tool result.
    #[must_use]
    pub fn error(tool_use_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: vec![ToolResultContent::Text {
                text: message.into(),
            }],
            is_error: Some(true),
        }
    }

    /// Create a tool result containing an image.
    #[must_use]
    pub fn with_image(
        tool_use_id: impl Into<String>,
        text: impl Into<String>,
        source: ImageSource,
    ) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: vec![
                ToolResultContent::Text { text: text.into() },
                ToolResultContent::Image { source },
            ],
            is_error: None,
        }
    }

    /// Collect all text parts into a single string.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                ToolResultContent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn input_schema_empty_roundtrip() {
        let schema = InputSchema::empty();
        let v = serde_json::to_value(&schema).unwrap();
        assert_eq!(v["type"], "object");
        let rt: InputSchema = serde_json::from_value(v).unwrap();
        assert_eq!(rt, schema);
    }

    #[test]
    fn input_schema_with_properties() {
        let schema = InputSchema::empty()
            .with_property("path", json!({"type": "string"}), true)
            .with_property("line", json!({"type": "integer"}), false);
        assert_eq!(schema.properties.len(), 2);
        assert_eq!(schema.required, vec!["path"]);
        let v = schema.to_value();
        assert_eq!(v["properties"]["path"]["type"], "string");
    }

    #[test]
    fn cached_tool_def_ephemeral_roundtrip() {
        let tool =
            CachedToolDefinition::ephemeral("read_file", "Read a file", json!({"type": "object"}));
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(v["cache_control"]["type"], "ephemeral");
        let rt: CachedToolDefinition = serde_json::from_value(v).unwrap();
        assert_eq!(rt, tool);
    }

    #[test]
    fn cached_tool_def_no_cache() {
        let tool = CachedToolDefinition {
            name: "noop".into(),
            description: "does nothing".into(),
            input_schema: json!({}),
            cache_control: None,
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert!(v.get("cache_control").is_none());
    }

    #[test]
    fn tool_result_content_text_roundtrip() {
        let c = ToolResultContent::Text {
            text: "output".into(),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["type"], "text");
        let rt: ToolResultContent = serde_json::from_value(v).unwrap();
        assert_eq!(rt, c);
    }

    #[test]
    fn tool_result_content_image_roundtrip() {
        let c = ToolResultContent::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc123".into(),
            },
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["type"], "image");
        assert_eq!(v["source"]["type"], "base64");
        let rt: ToolResultContent = serde_json::from_value(v).unwrap();
        assert_eq!(rt, c);
    }

    #[test]
    fn rich_tool_result_text_only() {
        let r = RichToolResult::text("toolu_01", "success");
        assert_eq!(r.text_content(), "success");
        assert!(r.is_error.is_none());
    }

    #[test]
    fn rich_tool_result_error() {
        let r = RichToolResult::error("toolu_02", "failed");
        assert_eq!(r.is_error, Some(true));
        assert_eq!(r.text_content(), "failed");
    }

    #[test]
    fn rich_tool_result_with_image() {
        let r = RichToolResult::with_image(
            "toolu_03",
            "screenshot captured",
            ImageSource::Base64 {
                media_type: "image/jpeg".into(),
                data: "JFIF...".into(),
            },
        );
        assert_eq!(r.content.len(), 2);
        assert_eq!(r.text_content(), "screenshot captured");
    }

    #[test]
    fn rich_tool_result_empty_content() {
        let r = RichToolResult {
            tool_use_id: "toolu_04".into(),
            content: vec![],
            is_error: None,
        };
        assert_eq!(r.text_content(), "");
        let v = serde_json::to_value(&r).unwrap();
        assert!(v.get("content").is_none()); // skip_serializing_if empty
    }

    #[test]
    fn rich_tool_result_serde_roundtrip() {
        let r = RichToolResult::with_image(
            "toolu_05",
            "data",
            ImageSource::Url {
                url: "https://img.example.com/x.png".into(),
            },
        );
        let v = serde_json::to_value(&r).unwrap();
        let rt: RichToolResult = serde_json::from_value(v).unwrap();
        assert_eq!(rt, r);
    }
}
