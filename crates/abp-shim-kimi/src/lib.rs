// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-shim-kimi
//!
//! Drop-in Kimi SDK shim that routes through ABP's intermediate representation.

/// Conversion layer between Kimi shim types and ABP core types.
pub mod convert;
/// HTTP client for the Moonshot (Kimi) Chat Completions API.
pub mod client;
/// Kimi-specific shim types (messages, usage, request builder).
pub mod types;

use std::pin::Pin;

use abp_core::{AgentEvent, Receipt, UsageNormalized, WorkOrder};
use abp_kimi_sdk::dialect::{KimiChunk, KimiRequest, KimiResponse};
use chrono::Utc;
use tokio_stream::Stream;

// Re-export key types from the Kimi SDK for convenience.
pub use abp_kimi_sdk::dialect::{
    KimiBuiltinFunction, KimiBuiltinTool, KimiFunctionDef, KimiRole, KimiTool, KimiToolDef,
};

// Re-export types and convert modules for backward compatibility.
pub use convert::*;
pub use types::*;

// ── Error types ─────────────────────────────────────────────────────────

/// Errors produced by the shim client.
#[derive(Debug, thiserror::Error)]
pub enum ShimError {
    /// The request was invalid.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// An internal processing error.
    #[error("internal error: {0}")]
    Internal(String),
    /// Serialization / deserialization error.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Result alias for shim operations.
pub type Result<T> = std::result::Result<T, ShimError>;

// ── Client types ────────────────────────────────────────────────────────

/// A callback function that processes a [`WorkOrder`] and returns a [`Receipt`].
pub type ProcessFn = Box<dyn Fn(&WorkOrder) -> Receipt + Send + Sync>;

/// Drop-in compatible Kimi client that routes through ABP.
pub struct KimiClient {
    model: String,
    processor: Option<ProcessFn>,
}

impl std::fmt::Debug for KimiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KimiClient")
            .field("model", &self.model)
            .finish()
    }
}

impl KimiClient {
    /// Create a new client targeting the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            processor: None,
        }
    }

    /// Set a custom processor function for handling work orders.
    #[must_use]
    pub fn with_processor(mut self, processor: ProcessFn) -> Self {
        self.processor = Some(processor);
        self
    }

    /// Get the configured model name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Create a chat completion (non-streaming).
    pub async fn create(&self, request: KimiRequest) -> Result<KimiResponse> {
        let work_order = request_to_work_order(&request);

        let receipt = if let Some(processor) = &self.processor {
            processor(&work_order)
        } else {
            return Err(ShimError::Internal(
                "no processor configured; use with_processor() to set a backend".into(),
            ));
        };

        Ok(receipt_to_response(&receipt, &request.model))
    }

    /// Create a streaming chat completion.
    pub async fn create_stream(
        &self,
        request: KimiRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = KimiChunk> + Send>>> {
        let work_order = request_to_work_order(&request);
        let model = request.model.clone();

        let receipt = if let Some(processor) = &self.processor {
            processor(&work_order)
        } else {
            return Err(ShimError::Internal(
                "no processor configured; use with_processor() to set a backend".into(),
            ));
        };

        let chunks = events_to_stream_chunks(&receipt.trace, &model);
        Ok(Box::pin(tokio_stream::iter(chunks)))
    }
}

// ── Test helpers ────────────────────────────────────────────────────────

/// Create a mock receipt for testing purposes.
#[must_use]
pub fn mock_receipt(events: Vec<AgentEvent>) -> Receipt {
    mock_receipt_with_usage(events, UsageNormalized::default())
}

/// Create a mock receipt with specified usage.
#[must_use]
pub fn mock_receipt_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    let run_id = uuid::Uuid::new_v4();
    Receipt {
        meta: abp_core::RunMetadata {
            run_id,
            work_order_id: uuid::Uuid::new_v4(),
            contract_version: abp_core::CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: abp_core::BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: Default::default(),
        mode: abp_core::ExecutionMode::Mapped,
        usage_raw: serde_json::Value::Null,
        usage,
        trace: events,
        artifacts: vec![],
        verification: Default::default(),
        outcome: abp_core::Outcome::Complete,
        receipt_sha256: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::ir::{IrRole, IrUsage};
    use abp_core::AgentEventKind;
    use serde_json::json;
    use tokio_stream::StreamExt;

    fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
        Box::new(move |_wo| mock_receipt(events.clone()))
    }

    fn make_processor_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> ProcessFn {
        Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
    }

    // ── 1. Simple chat completion roundtrip ─────────────────────────────

    #[tokio::test]
    async fn simple_chat_completion() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("Hi")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 2. Streaming chat completion ────────────────────────────────────

    #[tokio::test]
    async fn streaming_chat_completion() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "Hel".into() },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "lo!".into() },
                ext: None,
            },
        ];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .stream(true)
            .build();

        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<KimiChunk> = stream.collect().await;
        // 2 deltas + 1 final stop chunk
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hel"));
        assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("lo!"));
        assert_eq!(chunks[2].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 3. Tool use / function calling ──────────────────────────────────

    #[tokio::test]
    async fn tool_use_function_calling() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "web_search".into(),
                tool_use_id: Some("call_abc".into()),
                parent_tool_use_id: None,
                input: json!({"query": "rust async"}),
            },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Search for rust async")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_abc");
        assert_eq!(tcs[0].function.name, "web_search");
        assert!(tcs[0].function.arguments.contains("rust async"));
    }

    // ── 4. System message handling ──────────────────────────────────────

    #[tokio::test]
    async fn system_message_handling() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "I am helpful.".into(),
            },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![
                Message::system("You are a helpful assistant."),
                Message::user("Hello"),
            ])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("I am helpful.")
        );
    }

    // ── 5. Multi-turn conversation ──────────────────────────────────────

    #[tokio::test]
    async fn multi_turn_conversation() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "4".into() },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![
                Message::user("What is 2+2?"),
                Message::assistant("Let me calculate..."),
                Message::user("Just the number please"),
            ])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("4"));
    }

    // ── 6. Temperature mapping ──────────────────────────────────────────

    #[test]
    fn temperature_mapped_to_work_order() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("test")])
            .temperature(0.7)
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::Value::from(0.7))
        );
    }

    // ── 7. Max tokens mapping ───────────────────────────────────────────

    #[test]
    fn max_tokens_mapped_to_work_order() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("test")])
            .max_tokens(1024)
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("max_tokens"),
            Some(&serde_json::Value::from(1024))
        );
    }

    // ── 8. Model name preservation ──────────────────────────────────────

    #[tokio::test]
    async fn model_name_preserved_in_response() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "ok".into() },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-128k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.model, "moonshot-v1-128k");
    }

    // ── 9. Error response ───────────────────────────────────────────────

    #[tokio::test]
    async fn error_response() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "rate limit exceeded".into(),
                error_code: None,
            },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.create(req).await.unwrap();
        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limit exceeded"));
    }

    // ── 10. Token usage tracking ────────────────────────────────────────

    #[tokio::test]
    async fn token_usage_tracking() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        };
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "response".into(),
            },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-8k")
            .with_processor(make_processor_with_usage(events, usage));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.create(req).await.unwrap();
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    // ── 11. Request to IR roundtrip ─────────────────────────────────────

    #[test]
    fn request_to_ir_roundtrip() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::system("Be concise."), Message::user("Hello")])
            .build();

        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise.");
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[1].text_content(), "Hello");
    }

    // ── 12. Messages to IR and back ─────────────────────────────────────

    #[test]
    fn messages_to_ir_and_back() {
        let messages = vec![
            Message::system("System prompt"),
            Message::user("User message"),
            Message::assistant("Assistant reply"),
        ];

        let conv = messages_to_ir(&messages);
        let back = ir_to_messages(&conv);

        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("System prompt"));
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    // ── 13. No processor returns error ──────────────────────────────────

    #[tokio::test]
    async fn no_processor_returns_error() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    // ── 14. Builder defaults model ──────────────────────────────────────

    #[test]
    fn builder_defaults_model() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        assert_eq!(req.model, "moonshot-v1-8k");
    }

    // ── 15. Stream chunks end with stop ─────────────────────────────────

    #[test]
    fn stream_chunks_end_with_stop() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        }];

        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks.last().unwrap().choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }

    // ── 16. IR usage conversion ─────────────────────────────────────────

    #[test]
    fn ir_usage_converts_correctly() {
        let ir = IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    // ── 17. Request model maps to work order ────────────────────────────

    #[test]
    fn request_model_maps_to_work_order() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("test")])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    // ── 18. Multi-tool calls in response ────────────────────────────────

    #[tokio::test]
    async fn multi_tool_calls_in_response() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "search".into(),
                    tool_use_id: Some("call_1".into()),
                    parent_tool_use_id: None,
                    input: json!({"q": "a"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "search".into(),
                    tool_use_id: Some("call_2".into()),
                    parent_tool_use_id: None,
                    input: json!({"q": "b"}),
                },
                ext: None,
            },
        ];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Search")])
            .build();

        let resp = client.create(req).await.unwrap();
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].id, "call_1");
        assert_eq!(tcs[1].id, "call_2");
    }

    // ── 19. No processor stream returns error ───────────────────────────

    #[tokio::test]
    async fn no_processor_stream_returns_error() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let result = client.create_stream(req).await;
        assert!(result.is_err());
    }
}
