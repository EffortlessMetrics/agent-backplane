// SPDX-License-Identifier: MIT OR Apache-2.0
//! Anthropic Models API type definitions.
//!
//! Types for listing and retrieving model information via the
//! [Anthropic Models API](https://docs.anthropic.com/en/api/models).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Information about a single Anthropic model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Model {
    /// Model identifier (e.g. `claude-sonnet-4-20250514`).
    pub id: String,

    /// Object type — always `"model"`.
    #[serde(rename = "type")]
    pub object_type: String,

    /// Human-readable display name (e.g. `"Claude Sonnet 4"`).
    pub display_name: String,

    /// ISO 8601 timestamp of when the model was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Paginated response from the Anthropic Models listing endpoint.
///
/// Returned by `GET /v1/models`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ModelList {
    /// List of model objects.
    pub data: Vec<Model>,

    /// Whether there are more models available beyond this page.
    pub has_more: bool,

    /// ID of the first model in this page (for cursor-based pagination).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_id: Option<String>,

    /// ID of the last model in this page (for cursor-based pagination).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_serde_roundtrip() {
        let model = Model {
            id: "claude-sonnet-4-20250514".into(),
            object_type: "model".into(),
            display_name: "Claude Sonnet 4".into(),
            created_at: Some("2025-05-14T00:00:00Z".into()),
        };
        let json = serde_json::to_string(&model).unwrap();
        let parsed: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, model);
    }

    #[test]
    fn model_json_uses_type_field() {
        let model = Model {
            id: "claude-sonnet-4-20250514".into(),
            object_type: "model".into(),
            display_name: "Claude Sonnet 4".into(),
            created_at: None,
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(json["type"], "model");
        assert!(json.get("object_type").is_none());
    }

    #[test]
    fn model_omits_none_created_at() {
        let model = Model {
            id: "claude-sonnet-4-20250514".into(),
            object_type: "model".into(),
            display_name: "Claude Sonnet 4".into(),
            created_at: None,
        };
        let json = serde_json::to_string(&model).unwrap();
        assert!(!json.contains("created_at"));
    }

    #[test]
    fn model_list_serde_roundtrip() {
        let list = ModelList {
            data: vec![
                Model {
                    id: "claude-sonnet-4-20250514".into(),
                    object_type: "model".into(),
                    display_name: "Claude Sonnet 4".into(),
                    created_at: Some("2025-05-14T00:00:00Z".into()),
                },
                Model {
                    id: "claude-opus-4-20250514".into(),
                    object_type: "model".into(),
                    display_name: "Claude Opus 4".into(),
                    created_at: Some("2025-05-14T00:00:00Z".into()),
                },
            ],
            has_more: false,
            first_id: Some("claude-sonnet-4-20250514".into()),
            last_id: Some("claude-opus-4-20250514".into()),
        };
        let json = serde_json::to_string(&list).unwrap();
        let parsed: ModelList = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, list);
    }

    #[test]
    fn model_list_empty() {
        let list = ModelList {
            data: vec![],
            has_more: false,
            first_id: None,
            last_id: None,
        };
        let json = serde_json::to_string(&list).unwrap();
        let parsed: ModelList = serde_json::from_str(&json).unwrap();
        assert!(parsed.data.is_empty());
        assert!(!parsed.has_more);
    }

    #[test]
    fn model_json_schema_generates() {
        let schema = schemars::schema_for!(Model);
        let json = serde_json::to_value(&schema).unwrap();
        assert!(json.get("properties").is_some() || json.get("$defs").is_some());
    }

    #[test]
    fn model_list_json_schema_generates() {
        let schema = schemars::schema_for!(ModelList);
        let json = serde_json::to_value(&schema).unwrap();
        assert!(json.get("properties").is_some() || json.get("$defs").is_some());
    }
}
