// SPDX-License-Identifier: MIT OR Apache-2.0
//! OpenAI Embeddings API types.
//!
//! Covers the `/v1/embeddings` endpoint request and response wire format.

use serde::{Deserialize, Serialize};

/// Encoding format for returned embeddings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncodingFormat {
    /// Standard JSON array of floats.
    Float,
    /// Base64-encoded binary.
    Base64,
}

/// Input to the embeddings endpoint — single string or array of strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    /// A single string to embed.
    Single(String),
    /// Multiple strings to embed in a single request.
    Batch(Vec<String>),
}

/// Request body for the OpenAI Embeddings API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    /// Model identifier (e.g. `"text-embedding-3-small"`).
    pub model: String,
    /// Input text(s) to embed.
    pub input: EmbeddingInput,
    /// Encoding format for the output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<EncodingFormat>,
    /// The number of dimensions for the output embeddings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    /// A unique identifier representing your end-user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// A single embedding object in the response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingObject {
    /// Object type (always `"embedding"`).
    pub object: String,
    /// The embedding vector.
    pub embedding: Vec<f64>,
    /// Index of the input this embedding corresponds to.
    pub index: u32,
}

/// Token usage statistics for an embedding request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    /// Tokens in the input.
    pub prompt_tokens: u64,
    /// Total tokens consumed.
    pub total_tokens: u64,
}

/// Response body from the OpenAI Embeddings API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    /// Object type (always `"list"`).
    pub object: String,
    /// The embedding objects.
    pub data: Vec<EmbeddingObject>,
    /// Model used.
    pub model: String,
    /// Token usage statistics.
    pub usage: EmbeddingUsage,
}

// ── Builders ────────────────────────────────────────────────────────

impl EmbeddingRequest {
    /// Create a request to embed a single string.
    pub fn single(model: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            input: EmbeddingInput::Single(input.into()),
            encoding_format: None,
            dimensions: None,
            user: None,
        }
    }

    /// Create a request to embed multiple strings.
    pub fn batch(model: impl Into<String>, inputs: Vec<String>) -> Self {
        Self {
            model: model.into(),
            input: EmbeddingInput::Batch(inputs),
            encoding_format: None,
            dimensions: None,
            user: None,
        }
    }

    /// Set the encoding format.
    pub fn with_encoding_format(mut self, format: EncodingFormat) -> Self {
        self.encoding_format = Some(format);
        self
    }

    /// Set the desired dimensions.
    pub fn with_dimensions(mut self, dims: u32) -> Self {
        self.dimensions = Some(dims);
        self
    }

    /// Set the user identifier.
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── EmbeddingRequest serde ─────────────────────────────────────

    #[test]
    fn request_single_roundtrip() {
        let req = EmbeddingRequest::single("text-embedding-3-small", "Hello world");
        let json = serde_json::to_string(&req).unwrap();
        let back: EmbeddingRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn request_batch_roundtrip() {
        let req = EmbeddingRequest::batch(
            "text-embedding-3-large",
            vec!["Hello".into(), "World".into()],
        );
        let json = serde_json::to_string(&req).unwrap();
        let back: EmbeddingRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn request_with_all_options() {
        let req = EmbeddingRequest::single("model", "text")
            .with_encoding_format(EncodingFormat::Base64)
            .with_dimensions(256)
            .with_user("user-123");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("base64"));
        assert!(json.contains("256"));
        assert!(json.contains("user-123"));
        let back: EmbeddingRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn request_skips_none_fields() {
        let req = EmbeddingRequest::single("model", "text");
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("encoding_format"));
        assert!(!json.contains("dimensions"));
        assert!(!json.contains("user"));
    }

    // ── EmbeddingResponse serde ────────────────────────────────────

    #[test]
    fn response_roundtrip() {
        let resp = EmbeddingResponse {
            object: "list".into(),
            data: vec![EmbeddingObject {
                object: "embedding".into(),
                embedding: vec![0.1, 0.2, 0.3],
                index: 0,
            }],
            model: "text-embedding-3-small".into(),
            usage: EmbeddingUsage {
                prompt_tokens: 5,
                total_tokens: 5,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: EmbeddingResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_multiple_embeddings() {
        let resp = EmbeddingResponse {
            object: "list".into(),
            data: vec![
                EmbeddingObject {
                    object: "embedding".into(),
                    embedding: vec![0.1, 0.2],
                    index: 0,
                },
                EmbeddingObject {
                    object: "embedding".into(),
                    embedding: vec![0.3, 0.4],
                    index: 1,
                },
            ],
            model: "text-embedding-3-small".into(),
            usage: EmbeddingUsage {
                prompt_tokens: 10,
                total_tokens: 10,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: EmbeddingResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.data.len(), 2);
        assert_eq!(back.data[0].index, 0);
        assert_eq!(back.data[1].index, 1);
    }

    #[test]
    fn response_empty_data() {
        let resp = EmbeddingResponse {
            object: "list".into(),
            data: vec![],
            model: "model".into(),
            usage: EmbeddingUsage::default(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: EmbeddingResponse = serde_json::from_str(&json).unwrap();
        assert!(back.data.is_empty());
    }

    // ── EmbeddingObject serde ──────────────────────────────────────

    #[test]
    fn embedding_object_roundtrip() {
        let obj = EmbeddingObject {
            object: "embedding".into(),
            embedding: vec![-0.5, 0.0, 0.5, 1.0],
            index: 3,
        };
        let json = serde_json::to_string(&obj).unwrap();
        let back: EmbeddingObject = serde_json::from_str(&json).unwrap();
        assert_eq!(obj, back);
    }

    #[test]
    fn embedding_object_empty_vector() {
        let obj = EmbeddingObject {
            object: "embedding".into(),
            embedding: vec![],
            index: 0,
        };
        let json = serde_json::to_string(&obj).unwrap();
        let back: EmbeddingObject = serde_json::from_str(&json).unwrap();
        assert!(back.embedding.is_empty());
    }

    // ── EmbeddingUsage serde ───────────────────────────────────────

    #[test]
    fn embedding_usage_roundtrip() {
        let u = EmbeddingUsage {
            prompt_tokens: 42,
            total_tokens: 42,
        };
        let json = serde_json::to_string(&u).unwrap();
        let back: EmbeddingUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(u, back);
    }

    #[test]
    fn embedding_usage_default() {
        let u = EmbeddingUsage::default();
        assert_eq!(u.prompt_tokens, 0);
        assert_eq!(u.total_tokens, 0);
    }

    // ── EncodingFormat serde ───────────────────────────────────────

    #[test]
    fn encoding_format_roundtrip() {
        for fmt in [EncodingFormat::Float, EncodingFormat::Base64] {
            let json = serde_json::to_string(&fmt).unwrap();
            let back: EncodingFormat = serde_json::from_str(&json).unwrap();
            assert_eq!(fmt, back);
        }
    }

    #[test]
    fn encoding_format_values() {
        assert_eq!(serde_json::to_string(&EncodingFormat::Float).unwrap(), r#""float""#);
        assert_eq!(serde_json::to_string(&EncodingFormat::Base64).unwrap(), r#""base64""#);
    }

    // ── EmbeddingInput serde ───────────────────────────────────────

    #[test]
    fn embedding_input_single_serde() {
        let input = EmbeddingInput::Single("hello".into());
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, r#""hello""#);
        let back: EmbeddingInput = serde_json::from_str(&json).unwrap();
        assert_eq!(input, back);
    }

    #[test]
    fn embedding_input_batch_serde() {
        let input = EmbeddingInput::Batch(vec!["a".into(), "b".into()]);
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, r#"["a","b"]"#);
        let back: EmbeddingInput = serde_json::from_str(&json).unwrap();
        assert_eq!(input, back);
    }

    #[test]
    fn embedding_input_empty_batch() {
        let input = EmbeddingInput::Batch(vec![]);
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, "[]");
    }

    // ── Realistic API response ─────────────────────────────────────

    #[test]
    fn parse_realistic_api_response() {
        let json = r#"{
            "object": "list",
            "data": [
                {
                    "object": "embedding",
                    "embedding": [0.0023, -0.0094, 0.0156, -0.0078],
                    "index": 0
                }
            ],
            "model": "text-embedding-3-small",
            "usage": {
                "prompt_tokens": 8,
                "total_tokens": 8
            }
        }"#;
        let resp: EmbeddingResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 1);
        assert_eq!(resp.data[0].embedding.len(), 4);
        assert_eq!(resp.usage.prompt_tokens, 8);
    }
}
