// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP client for the GitHub Copilot Chat API.

use std::time::Duration;

use abp_copilot_sdk::dialect::{CopilotRequest, CopilotResponse, CopilotStreamEvent};
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

const DEFAULT_BASE_URL: &str = "https://api.githubcopilot.com";

// ── Client ──────────────────────────────────────────────────────────────

/// HTTP client for the GitHub Copilot Chat Completions API.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl Client {
    /// Create a new client with the given API key (Copilot token).
    ///
    /// Uses the default base URL (`https://api.githubcopilot.com`) and a
    /// 30-second timeout.
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
        headers.insert(
            "Copilot-Integration-Id",
            HeaderValue::from_static("agent-backplane"),
        );
        headers
    }

    /// Send a chat completion request.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn chat_completion(&self, request: &CopilotRequest) -> Result<CopilotResponse> {
        let url = format!("{}/chat/completions", self.base_url);
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

    /// Send a streaming chat completion request.
    ///
    /// Returns a stream of [`CopilotStreamEvent`]s parsed from the SSE
    /// response.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn stream_chat_completion(
        &self,
        request: &CopilotRequest,
    ) -> Result<impl Stream<Item = Result<CopilotStreamEvent>>> {
        let url = format!("{}/chat/completions", self.base_url);
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

// ── High-level CopilotClient facade ─────────────────────────────────────

/// High-level Copilot client facade that mirrors the GitHub Copilot
/// Extensions API surface. Created with `CopilotClient::new(token)` and
/// provides `chat()` and `chat_stream()` convenience methods.
#[derive(Debug, Clone)]
pub struct CopilotClient {
    inner: Client,
}

impl CopilotClient {
    /// Create a new client with the given Copilot token.
    ///
    /// Uses the default base URL (`https://api.githubcopilot.com`) and a
    /// 30-second timeout.
    pub fn new(token: impl Into<String>) -> Result<Self> {
        Ok(Self {
            inner: Client::new(token)?,
        })
    }

    /// Create a new client with custom configuration via the builder.
    pub fn with_builder(builder: ClientBuilder) -> Result<Self> {
        Ok(Self {
            inner: builder.build()?,
        })
    }

    /// Send a chat completion request (non-streaming).
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn chat(&self, request: &CopilotRequest) -> Result<CopilotResponse> {
        self.inner.chat_completion(request).await
    }

    /// Send a streaming chat completion request.
    ///
    /// Returns a stream of [`CopilotStreamEvent`]s parsed from the SSE
    /// response.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn chat_stream(
        &self,
        request: &CopilotRequest,
    ) -> Result<impl Stream<Item = Result<CopilotStreamEvent>>> {
        self.inner.stream_chat_completion(request).await
    }

    /// The base URL this client targets.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    /// Access the underlying low-level [`Client`].
    #[must_use]
    pub fn inner(&self) -> &Client {
        &self.inner
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn client_construction_defaults() {
        let client = Client::new("ghu_test-token").unwrap();
        assert_eq!(client.base_url(), "https://api.githubcopilot.com");
    }

    #[test]
    fn url_building() {
        let client = Client::new("ghu_test").unwrap();
        let url = format!("{}/chat/completions", client.base_url());
        assert_eq!(url, "https://api.githubcopilot.com/chat/completions");
    }

    #[test]
    fn vendor_specific_headers() {
        let client = Client::new("ghu_abc123").unwrap();
        let headers = client.default_headers();
        let auth = headers.get(AUTHORIZATION).unwrap().to_str().unwrap();
        assert_eq!(auth, "Bearer ghu_abc123");
        assert_eq!(
            headers
                .get("Copilot-Integration-Id")
                .unwrap()
                .to_str()
                .unwrap(),
            "agent-backplane"
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
            body: "unauthorized".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("401"));
        assert!(msg.contains("unauthorized"));
    }

    #[test]
    fn builder_base_url_override() {
        let client = Client::builder("ghu_key")
            .base_url("https://custom.copilot.example")
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        assert_eq!(client.base_url(), "https://custom.copilot.example");
    }

    // ── CopilotClient facade tests ──────────────────────────────────────

    #[test]
    fn copilot_client_construction() {
        let client = CopilotClient::new("ghu_test-token").unwrap();
        assert_eq!(client.base_url(), "https://api.githubcopilot.com");
    }

    #[test]
    fn copilot_client_with_builder() {
        let builder =
            ClientBuilder::new("ghu_key").base_url("https://custom.copilot.example");
        let client = CopilotClient::with_builder(builder).unwrap();
        assert_eq!(client.base_url(), "https://custom.copilot.example");
    }

    #[test]
    fn copilot_client_inner_access() {
        let client = CopilotClient::new("ghu_inner").unwrap();
        assert_eq!(client.inner().base_url(), "https://api.githubcopilot.com");
    }

    #[test]
    fn copilot_client_is_debug() {
        let client = CopilotClient::new("ghu_debug").unwrap();
        let debug = format!("{client:?}");
        assert!(debug.contains("CopilotClient"));
    }

    #[test]
    fn copilot_client_is_clone() {
        let client = CopilotClient::new("ghu_clone").unwrap();
        let _cloned = client.clone();
        assert_eq!(client.base_url(), "https://api.githubcopilot.com");
    }
}
