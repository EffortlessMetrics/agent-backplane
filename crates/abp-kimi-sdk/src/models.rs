// SPDX-License-Identifier: MIT OR Apache-2.0
//! Model listing types for the Moonshot Kimi `/v1/models` endpoint.
//!
//! These types model the response from `GET /v1/models`, matching the
//! Moonshot REST API surface (OpenAI-compatible).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Model object
// ---------------------------------------------------------------------------

/// A single model object returned by the Kimi Models API.
///
/// Corresponds to the JSON object in `/v1/models` responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct KimiModel {
    /// The model identifier (e.g. `moonshot-v1-8k`, `k1`).
    pub id: String,
    /// Object type — always `"model"`.
    pub object: String,
    /// Unix timestamp (seconds) when the model was created.
    pub created: u64,
    /// The organization that owns the model.
    pub owned_by: String,
}

// ---------------------------------------------------------------------------
// Model list response
// ---------------------------------------------------------------------------

/// Response from `GET /v1/models`.
///
/// Contains a list of available Kimi models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct KimiModelList {
    /// Object type — always `"list"`.
    pub object: String,
    /// The list of model objects.
    pub data: Vec<KimiModel>,
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

impl KimiModel {
    /// Create a new model object with the given id and owner.
    #[must_use]
    pub fn new(id: impl Into<String>, owned_by: impl Into<String>, created: u64) -> Self {
        Self {
            id: id.into(),
            object: "model".into(),
            created,
            owned_by: owned_by.into(),
        }
    }
}

impl KimiModelList {
    /// Create a model list from a vec of models.
    #[must_use]
    pub fn new(models: Vec<KimiModel>) -> Self {
        Self {
            object: "list".into(),
            data: models,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_serde_roundtrip() {
        let model = KimiModel::new("moonshot-v1-8k", "moonshot", 1700000000);
        let json = serde_json::to_string(&model).unwrap();
        let parsed: KimiModel = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, model);
        assert_eq!(parsed.object, "model");
    }

    #[test]
    fn model_list_serde_roundtrip() {
        let list = KimiModelList::new(vec![
            KimiModel::new("moonshot-v1-8k", "moonshot", 1700000000),
            KimiModel::new("moonshot-v1-128k", "moonshot", 1700000001),
            KimiModel::new("k1", "moonshot", 1700000002),
        ]);
        let json = serde_json::to_string(&list).unwrap();
        let parsed: KimiModelList = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.data.len(), 3);
        assert_eq!(parsed.object, "list");
    }

    #[test]
    fn model_list_empty() {
        let list = KimiModelList::new(vec![]);
        let json = serde_json::to_string(&list).unwrap();
        let parsed: KimiModelList = serde_json::from_str(&json).unwrap();
        assert!(parsed.data.is_empty());
    }

    #[test]
    fn model_deserializes_from_api_json() {
        let json = r#"{
            "id": "moonshot-v1-8k",
            "object": "model",
            "created": 1715367049,
            "owned_by": "moonshot"
        }"#;
        let model: KimiModel = serde_json::from_str(json).unwrap();
        assert_eq!(model.id, "moonshot-v1-8k");
        assert_eq!(model.owned_by, "moonshot");
    }

    #[test]
    fn model_list_deserializes_from_api_json() {
        let json = r#"{
            "object": "list",
            "data": [
                {"id": "moonshot-v1-8k", "object": "model", "created": 1715367049, "owned_by": "moonshot"},
                {"id": "moonshot-v1-32k", "object": "model", "created": 1715367049, "owned_by": "moonshot"},
                {"id": "moonshot-v1-128k", "object": "model", "created": 1715367049, "owned_by": "moonshot"}
            ]
        }"#;
        let list: KimiModelList = serde_json::from_str(json).unwrap();
        assert_eq!(list.data.len(), 3);
        assert_eq!(list.data[0].id, "moonshot-v1-8k");
        assert_eq!(list.data[2].id, "moonshot-v1-128k");
    }
}
