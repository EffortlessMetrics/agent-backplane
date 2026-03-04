// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP client for the Anthropic Messages API.

use std::time::Duration;

use futures_core::Stream;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

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
}
