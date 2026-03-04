// SPDX-License-Identifier: MIT OR Apache-2.0
//! Model listing types for the OpenAI `/v1/models` endpoint.
//!
//! These types model the response from `GET /v1/models` and
//! `GET /v1/models/{model}`, matching the OpenAI REST API surface.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Model object
// ---------------------------------------------------------------------------

/// A single model object returned by the OpenAI Models API.
///
/// Corresponds to the JSON object in `/v1/models` responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Model {
    /// The model identifier (e.g. `gpt-4o`, `gpt-4-turbo`).
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
/// Contains a paginated list of available models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ModelList {
    /// Object type — always `"list"`.
    pub object: String,
    /// The list of model objects.
    pub data: Vec<Model>,
}

// ---------------------------------------------------------------------------
// Model deletion response
// ---------------------------------------------------------------------------

/// Response from `DELETE /v1/models/{model}` (fine-tuned models only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ModelDeleted {
    /// The model identifier that was deleted.
    pub id: String,
    /// Object type — always `"model"`.
    pub object: String,
    /// Whether the deletion was successful.
    pub deleted: bool,
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

impl Model {
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

impl ModelList {
    /// Create a model list from a vec of models.
    #[must_use]
    pub fn new(models: Vec<Model>) -> Self {
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
        let model = Model::new("gpt-4o", "openai", 1700000000);
        let json = serde_json::to_string(&model).unwrap();
        let parsed: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, model);
        assert_eq!(parsed.object, "model");
    }

    #[test]
    fn model_list_serde_roundtrip() {
        let list = ModelList::new(vec![
            Model::new("gpt-4o", "openai", 1700000000),
            Model::new("gpt-4o-mini", "openai", 1700000001),
        ]);
        let json = serde_json::to_string(&list).unwrap();
        let parsed: ModelList = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.object, "list");
    }

    #[test]
    fn model_deleted_serde_roundtrip() {
        let deleted = ModelDeleted {
            id: "ft:gpt-4o:my-org:custom:abc".into(),
            object: "model".into(),
            deleted: true,
        };
        let json = serde_json::to_string(&deleted).unwrap();
        let parsed: ModelDeleted = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, deleted);
        assert!(parsed.deleted);
    }

    #[test]
    fn model_list_empty() {
        let list = ModelList::new(vec![]);
        let json = serde_json::to_string(&list).unwrap();
        let parsed: ModelList = serde_json::from_str(&json).unwrap();
        assert!(parsed.data.is_empty());
    }

    #[test]
    fn model_deserializes_from_openai_json() {
        let json = r#"{
            "id": "gpt-4o",
            "object": "model",
            "created": 1715367049,
            "owned_by": "system"
        }"#;
        let model: Model = serde_json::from_str(json).unwrap();
        assert_eq!(model.id, "gpt-4o");
        assert_eq!(model.owned_by, "system");
    }

    #[test]
    fn model_list_deserializes_from_openai_json() {
        let json = r#"{
            "object": "list",
            "data": [
                {"id": "gpt-4o", "object": "model", "created": 1715367049, "owned_by": "system"},
                {"id": "gpt-4o-mini", "object": "model", "created": 1721172741, "owned_by": "system"}
            ]
        }"#;
        let list: ModelList = serde_json::from_str(json).unwrap();
        assert_eq!(list.data.len(), 2);
        assert_eq!(list.data[0].id, "gpt-4o");
        assert_eq!(list.data[1].id, "gpt-4o-mini");
    }
}
