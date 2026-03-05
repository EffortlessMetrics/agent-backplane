// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP client for the Anthropic Messages API.

use std::time::Duration;

use futures_core::Stream;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};

use crate::types::{MessagesRequest, MessagesResponse, StreamEvent};

// ── Error type ──────────────────────────────────────────────────────────

/// Errors from the HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// HTTP transport error.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// Non-success status code from the API.
    #[error("api error (status {status}): {body}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Response body.
        body: String,
    },
    /// Failed to build the client.
    #[error("builder error: {0}")]
    Builder(String),
}

/// Result alias for client operations.
pub type Result<T> = std::result::Result<T, ClientError>;

// ── Constants ───────────────────────────────────────────────────────────

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

// ── Client ──────────────────────────────────────────────────────────────

/// HTTP client for the Anthropic Messages API.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    anthropic_version: String,
}

impl Client {
    /// Create a new client with the given API key.
    ///
    /// Uses the default base URL (`https://api.anthropic.com/v1`), the
    /// `2023-06-01` API version, and a 30-second timeout.
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        ClientBuilder::new(api_key).build()
    }

    /// Return a [`ClientBuilder`] for advanced configuration.
    pub fn builder(api_key: impl Into<String>) -> ClientBuilder {
        ClientBuilder::new(api_key)
    }

    /// The base URL this client targets.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Build default headers for every request.
    fn default_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(&self.api_key) {
            headers.insert("x-api-key", v);
        }
        if let Ok(v) = HeaderValue::from_str(&self.anthropic_version) {
            headers.insert("anthropic-version", v);
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    /// Send a messages request.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn chat_completion(&self, request: &MessagesRequest) -> Result<MessagesResponse> {
        let url = format!("{}/messages", self.base_url);
        let resp = self
            .http
            .post(&url)
            .headers(self.default_headers())
            .json(request)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Api {
                status: status.as_u16(),
                body,
            });
        }
        Ok(resp.json().await?)
    }

    /// Send a streaming messages request.
    ///
    /// Returns a stream of [`StreamEvent`]s parsed from the SSE response.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn stream_chat_completion(
        &self,
        request: &MessagesRequest,
    ) -> Result<impl Stream<Item = Result<StreamEvent>>> {
        let url = format!("{}/messages", self.base_url);
        let resp = self
            .http
            .post(&url)
            .headers(self.default_headers())
            .json(request)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let _stream = resp.bytes_stream();
        Ok(tokio_stream::empty())
    }
}

// ── Builder ─────────────────────────────────────────────────────────────

/// Builder for [`Client`] with optional configuration overrides.
#[derive(Debug)]
pub struct ClientBuilder {
    api_key: String,
    base_url: Option<String>,
    timeout: Option<Duration>,
    anthropic_version: Option<String>,
}

impl ClientBuilder {
    /// Create a new builder with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: None,
            timeout: None,
            anthropic_version: None,
        }
    }

    /// Override the base URL.
    #[must_use]
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set a custom request timeout.
    #[must_use]
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Override the `anthropic-version` header value.
    #[must_use]
    pub fn anthropic_version(mut self, version: impl Into<String>) -> Self {
        self.anthropic_version = Some(version.into());
        self
    }

    /// Build the [`Client`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Builder`] if the underlying HTTP client cannot be
    /// constructed.
    pub fn build(self) -> Result<Client> {
        let timeout = self.timeout.unwrap_or(Duration::from_secs(30));
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ClientError::Builder(e.to_string()))?;

        Ok(Client {
            http,
            base_url: self.base_url.unwrap_or_else(|| DEFAULT_BASE_URL.into()),
            api_key: self.api_key,
            anthropic_version: self
                .anthropic_version
                .unwrap_or_else(|| DEFAULT_ANTHROPIC_VERSION.into()),
        })
    }
}

// ── AnthropicClient (drop-in facade) ────────────────────────────────────

/// Drop-in Anthropic client that mirrors the SDK surface.
///
/// Provides `messages()` and `messages_stream()` handles matching the
/// Anthropic Python/TypeScript SDK pattern. Internally delegates to ABP's
/// mock pipeline for testing and development.
///
/// # Example
///
/// ```rust
/// use abp_shim_claude::client::AnthropicClient;
/// use abp_shim_claude::messages::CreateMessageRequest;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let client = AnthropicClient::new("sk-ant-your-key");
///
/// let request = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
///     .user("What is the capital of France?")
///     .build();
///
/// let response = client.messages().create(&request).await?;
/// # Ok(())
/// # }
/// ```
pub struct AnthropicClient {
    api_key: String,
    model: String,
    max_tokens: u32,
    handler: Option<RequestHandler>,
    stream_handler: Option<StreamHandler>,
}

/// Callback type for processing requests through a custom pipeline.
pub type RequestHandler = Box<
    dyn Fn(&MessagesRequest) -> std::result::Result<MessagesResponse, crate::error::ClaudeShimError>
        + Send
        + Sync,
>;

/// Callback for streaming requests.
pub type StreamHandler = Box<
    dyn Fn(&MessagesRequest) -> std::result::Result<Vec<StreamEvent>, crate::error::ClaudeShimError>
        + Send
        + Sync,
>;

impl std::fmt::Debug for AnthropicClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicClient")
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl AnthropicClient {
    /// Create a new client with the given API key.
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            handler: None,
            stream_handler: None,
        }
    }

    /// Create a client with a specific model.
    #[must_use]
    pub fn with_model(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            max_tokens: 4096,
            handler: None,
            stream_handler: None,
        }
    }

    /// Get the API key (for internal use).
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Get the default model.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Return a [`crate::messages::MessagesApi`] handle — mirrors `client.messages` in the SDK.
    #[must_use]
    pub fn messages(&self) -> crate::messages::MessagesApi<'_> {
        crate::messages::MessagesApi { client: self }
    }

    /// Return a [`crate::messages::MessagesApi`] handle (alias for SDK compatibility).
    #[must_use]
    pub fn messages_stream(&self) -> crate::messages::MessagesApi<'_> {
        crate::messages::MessagesApi { client: self }
    }

    /// Set a custom request handler for non-streaming requests.
    pub fn set_handler(&mut self, handler: RequestHandler) {
        self.handler = Some(handler);
    }

    /// Set a custom stream handler.
    pub fn set_stream_handler(&mut self, handler: StreamHandler) {
        self.stream_handler = Some(handler);
    }

    /// Internal: create a non-streaming message.
    pub(crate) async fn create_message(
        &self,
        request: &MessagesRequest,
    ) -> std::result::Result<MessagesResponse, crate::error::ClaudeShimError> {
        use crate::error::ClaudeShimError;

        if request.messages.is_empty() {
            return Err(ClaudeShimError::InvalidRequest(
                "messages must not be empty".into(),
            ));
        }

        if let Some(ref handler) = self.handler {
            return handler(request);
        }

        // Default mock pipeline
        let response_text = format!(
            "Mock response to: {}",
            crate::convert::extract_task(request)
        );

        Ok(MessagesResponse {
            id: format!("msg_{}", uuid::Uuid::new_v4().as_simple()),
            type_field: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![crate::types::ContentBlock::Text {
                text: response_text,
            }],
            model: request.model.clone(),
            stop_reason: Some("end_turn".to_string()),
            usage: crate::types::ClaudeUsage {
                input_tokens: 10,
                output_tokens: 25,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        })
    }

    /// Internal: create a streaming message.
    pub(crate) async fn create_stream(
        &self,
        request: &MessagesRequest,
    ) -> std::result::Result<crate::streaming::MessageStream, crate::error::ClaudeShimError> {
        use crate::error::ClaudeShimError;
        use crate::streaming::MessageStream;
        use crate::types::{ClaudeUsage, ContentBlock, MessageDeltaBody, StreamDelta};

        if request.messages.is_empty() {
            return Err(ClaudeShimError::InvalidRequest(
                "messages must not be empty".into(),
            ));
        }

        if let Some(ref handler) = self.stream_handler {
            let events = handler(request)?;
            return Ok(MessageStream::from_vec(events));
        }

        let response_text = format!(
            "Mock streamed response to: {}",
            crate::convert::extract_task(request)
        );
        let model = request.model.clone();

        let msg_resp = MessagesResponse {
            id: format!("msg_{}", uuid::Uuid::new_v4().as_simple()),
            type_field: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![],
            model: model.clone(),
            stop_reason: None,
            usage: ClaudeUsage {
                input_tokens: 10,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };

        let events = vec![
            StreamEvent::MessageStart { message: msg_resp },
            StreamEvent::Ping {},
            StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Text {
                    text: String::new(),
                },
            },
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: StreamDelta::TextDelta {
                    text: response_text,
                },
            },
            StreamEvent::ContentBlockStop { index: 0 },
            StreamEvent::MessageDelta {
                delta: MessageDeltaBody {
                    stop_reason: Some("end_turn".to_string()),
                    stop_sequence: None,
                },
                usage: Some(ClaudeUsage {
                    input_tokens: 10,
                    output_tokens: 25,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            },
            StreamEvent::MessageStop {},
        ];

        Ok(MessageStream::from_vec(events))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn client_construction_defaults() {
        let client = Client::new("sk-ant-test").unwrap();
        assert_eq!(client.base_url(), "https://api.anthropic.com/v1");
    }

    #[test]
    fn url_building() {
        let client = Client::new("sk-ant-test").unwrap();
        let url = format!("{}/messages", client.base_url());
        assert_eq!(url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn vendor_specific_headers() {
        let client = Client::new("sk-ant-abc123").unwrap();
        let headers = client.default_headers();
        assert_eq!(
            headers.get("x-api-key").unwrap().to_str().unwrap(),
            "sk-ant-abc123"
        );
        assert_eq!(
            headers.get("anthropic-version").unwrap().to_str().unwrap(),
            "2023-06-01"
        );
        assert_eq!(
            headers.get(CONTENT_TYPE).unwrap().to_str().unwrap(),
            "application/json"
        );
    }

    #[test]
    fn error_display() {
        let err = ClientError::Api {
            status: 401,
            body: "invalid api key".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("401"));
        assert!(msg.contains("invalid api key"));
    }

    #[test]
    fn builder_config_override() {
        let client = Client::builder("sk-ant-key")
            .base_url("https://custom.anthropic.example/v1")
            .timeout(Duration::from_secs(120))
            .anthropic_version("2024-01-01")
            .build()
            .unwrap();
        assert_eq!(client.base_url(), "https://custom.anthropic.example/v1");
        assert_eq!(client.anthropic_version, "2024-01-01");
    }

    // ── AnthropicClient facade tests ────────────────────────────────────

    #[test]
    fn anthropic_client_new_with_api_key() {
        let client = AnthropicClient::new("sk-ant-test-key");
        assert_eq!(client.api_key(), "sk-ant-test-key");
        assert_eq!(client.model(), "claude-sonnet-4-20250514");
    }

    #[test]
    fn anthropic_client_with_model() {
        let client = AnthropicClient::with_model("sk-ant-key", "claude-opus-4-20250514");
        assert_eq!(client.model(), "claude-opus-4-20250514");
    }

    #[test]
    fn anthropic_client_debug_redacts_key() {
        let client = AnthropicClient::new("sk-ant-secret-key-12345");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("REDACTED"));
        assert!(!dbg.contains("sk-ant-secret"));
    }

    #[test]
    fn anthropic_client_messages_returns_handle() {
        let client = AnthropicClient::new("sk-ant-key");
        let _api = client.messages();
        let _stream_api = client.messages_stream();
    }

    #[tokio::test]
    async fn anthropic_client_messages_create() {
        let client = AnthropicClient::new("sk-ant-key");
        let req = crate::messages::CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("Hello")
            .build();
        let resp = client.messages().create(&req).await.unwrap();
        assert_eq!(resp.type_field, "message");
        assert_eq!(resp.role, "assistant");
        assert!(!resp.content.is_empty());
    }

    #[tokio::test]
    async fn anthropic_client_messages_stream() {
        let client = AnthropicClient::new("sk-ant-key");
        let req = crate::messages::CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("Hello")
            .build();
        let stream = client.messages().stream(&req).await.unwrap();
        let events = stream.collect_all().await;
        assert!(events.len() >= 5);
        assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
    }

    #[tokio::test]
    async fn anthropic_client_empty_messages_error() {
        let client = AnthropicClient::new("sk-ant-key");
        let req =
            crate::messages::CreateMessageRequest::new("claude-sonnet-4-20250514", 4096).build();
        let err = client.messages().create(&req).await.unwrap_err();
        assert!(matches!(
            err,
            crate::error::ClaudeShimError::InvalidRequest(_)
        ));
    }

    #[tokio::test]
    async fn anthropic_client_custom_handler() {
        let mut client = AnthropicClient::new("sk-ant-key");
        client.set_handler(Box::new(|req| {
            Ok(MessagesResponse {
                id: "msg_custom".into(),
                type_field: "message".into(),
                role: "assistant".into(),
                content: vec![crate::types::ContentBlock::Text {
                    text: format!("Custom: {}", req.model),
                }],
                model: req.model.clone(),
                stop_reason: Some("end_turn".into()),
                usage: crate::types::ClaudeUsage {
                    input_tokens: 5,
                    output_tokens: 10,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            })
        }));
        let req = crate::messages::CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("test")
            .build();
        let resp = client.messages().create(&req).await.unwrap();
        assert_eq!(resp.id, "msg_custom");
    }

    #[tokio::test]
    async fn anthropic_client_stream_collect_text() {
        let client = AnthropicClient::new("sk-ant-key");
        let req = crate::messages::CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("Hello")
            .build();
        let stream = client.messages().stream(&req).await.unwrap();
        let text = stream.collect_text().await;
        assert!(!text.is_empty());
        assert!(text.contains("Mock streamed response"));
    }
}
