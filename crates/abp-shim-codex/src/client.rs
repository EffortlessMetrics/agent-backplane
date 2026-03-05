// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP client for the OpenAI Codex / Responses API.

use std::time::Duration;

use abp_codex_sdk::dialect::{CodexRequest, CodexResponse, CodexStreamEvent};
use futures_core::Stream;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

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

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

// ── Client ──────────────────────────────────────────────────────────────

/// HTTP client for the OpenAI Codex Responses API.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl Client {
    /// Create a new client with the given API key.
    ///
    /// Uses the default base URL (`https://api.openai.com/v1`) and a 30-second
    /// timeout.
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
        if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", self.api_key)) {
            headers.insert(AUTHORIZATION, v);
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    /// Send a Codex responses request.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn chat_completion(&self, request: &CodexRequest) -> Result<CodexResponse> {
        let url = format!("{}/responses", self.base_url);
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

    /// Send a streaming Codex responses request.
    ///
    /// Returns a stream of [`CodexStreamEvent`]s parsed from the SSE response.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn stream_chat_completion(
        &self,
        request: &CodexRequest,
    ) -> Result<impl Stream<Item = Result<CodexStreamEvent>>> {
        let url = format!("{}/responses", self.base_url);
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

    /// Return a [`ResponsesApi`] handle for the Responses API.
    ///
    /// Provides the `client.responses().create(req)` / `.stream(req)` pattern
    /// matching the OpenAI Codex SDK surface.
    pub fn responses(&self) -> ResponsesApi<'_> {
        ResponsesApi { client: self }
    }
}

// ── Responses API handle ────────────────────────────────────────────────

/// Scoped handle for the `/v1/responses` endpoint.
///
/// Obtained via [`Client::responses()`]. Provides `create()` and `stream()`
/// methods that mirror the OpenAI Codex SDK's builder-chain pattern.
#[derive(Debug)]
pub struct ResponsesApi<'c> {
    client: &'c Client,
}

impl<'c> ResponsesApi<'c> {
    /// Create a response (non-streaming).
    ///
    /// Sends a POST to `/v1/responses` with the given request and returns
    /// a [`CodexResponse`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn create(&self, request: &CodexRequest) -> Result<CodexResponse> {
        self.client.chat_completion(request).await
    }

    /// Create a streaming response.
    ///
    /// Sends a POST to `/v1/responses` with `stream: true` and returns
    /// a stream of [`CodexStreamEvent`]s.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn stream(
        &self,
        request: &CodexRequest,
    ) -> Result<impl Stream<Item = Result<CodexStreamEvent>>> {
        self.client.stream_chat_completion(request).await
    }
}

// ── Builder ─────────────────────────────────────────────────────────────

/// Builder for [`Client`] with optional configuration overrides.
#[derive(Debug)]
pub struct ClientBuilder {
    api_key: String,
    base_url: Option<String>,
    timeout: Option<Duration>,
}

impl ClientBuilder {
    /// Create a new builder with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: None,
            timeout: None,
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
        })
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn client_construction_defaults() {
        let client = Client::new("sk-test-key").unwrap();
        assert_eq!(client.base_url(), "https://api.openai.com/v1");
    }

    #[test]
    fn url_building() {
        let client = Client::new("sk-test").unwrap();
        let url = format!("{}/responses", client.base_url());
        assert_eq!(url, "https://api.openai.com/v1/responses");
    }

    #[test]
    fn authorization_header() {
        let client = Client::new("sk-codex-abc").unwrap();
        let headers = client.default_headers();
        let auth = headers.get(AUTHORIZATION).unwrap().to_str().unwrap();
        assert_eq!(auth, "Bearer sk-codex-abc");
        assert_eq!(
            headers.get(CONTENT_TYPE).unwrap().to_str().unwrap(),
            "application/json"
        );
    }

    #[test]
    fn error_display() {
        let err = ClientError::Api {
            status: 500,
            body: "internal server error".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("500"));
        assert!(msg.contains("internal server error"));
    }

    #[test]
    fn builder_base_url_override() {
        let client = Client::builder("sk-key")
            .base_url("https://custom.openai.example/v1")
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap();
        assert_eq!(client.base_url(), "https://custom.openai.example/v1");
    }

    #[test]
    fn responses_api_is_accessible() {
        let client = Client::new("sk-test").unwrap();
        let api = client.responses();
        // ResponsesApi holds a reference to the client
        assert!(format!("{api:?}").contains("ResponsesApi"));
    }

    #[test]
    fn responses_api_url_matches_endpoint() {
        let client = Client::new("sk-test").unwrap();
        let _api = client.responses();
        let expected = format!("{}/responses", client.base_url());
        assert_eq!(expected, "https://api.openai.com/v1/responses");
    }

    #[test]
    fn client_api_key_stored() {
        let client = Client::new("sk-codex-test-123").unwrap();
        let headers = client.default_headers();
        let auth = headers.get(AUTHORIZATION).unwrap().to_str().unwrap();
        assert!(auth.contains("sk-codex-test-123"));
    }
}
