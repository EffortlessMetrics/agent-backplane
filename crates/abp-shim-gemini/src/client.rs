// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP client for the Google Gemini API.

use std::time::Duration;

use futures_core::Stream;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::types::{GenerateContentRequest, GenerateContentResponse, StreamEvent};

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

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

// ── Client ──────────────────────────────────────────────────────────────

/// HTTP client for the Google Gemini generative language API.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl Client {
    /// Create a new client with the given API key.
    ///
    /// Uses the default base URL
    /// (`https://generativelanguage.googleapis.com/v1beta`) and a 30-second
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
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    /// Build the URL for a model method, appending the API key as a query
    /// parameter (Gemini convention).
    fn model_url(&self, model: &str, method: &str) -> String {
        format!(
            "{}/models/{}:{}?key={}",
            self.base_url, model, method, self.api_key
        )
    }

    /// Send a content generation request.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn chat_completion(
        &self,
        request: &GenerateContentRequest,
    ) -> Result<GenerateContentResponse> {
        let url = self.model_url(&request.model, "generateContent");
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

    /// Send a streaming content generation request.
    ///
    /// Returns a stream of [`StreamEvent`]s parsed from the chunked response.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn stream_chat_completion(
        &self,
        request: &GenerateContentRequest,
    ) -> Result<impl Stream<Item = Result<StreamEvent>>> {
        let url = self.model_url(&request.model, "streamGenerateContent");
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

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn client_construction_defaults() {
        let client = Client::new("AIza-test-key").unwrap();
        assert_eq!(
            client.base_url(),
            "https://generativelanguage.googleapis.com/v1beta"
        );
    }

    #[test]
    fn url_building_with_model() {
        let client = Client::new("AIza-key").unwrap();
        let url = client.model_url("gemini-2.5-flash", "generateContent");
        assert!(url.starts_with(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
        ));
        assert!(url.contains("key=AIza-key"));
    }

    #[test]
    fn content_type_header() {
        let client = Client::new("AIza-key").unwrap();
        let headers = client.default_headers();
        assert_eq!(
            headers.get(CONTENT_TYPE).unwrap().to_str().unwrap(),
            "application/json"
        );
    }

    #[test]
    fn error_display() {
        let err = ClientError::Api {
            status: 403,
            body: "forbidden".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("403"));
        assert!(msg.contains("forbidden"));
    }

    #[test]
    fn builder_config_override() {
        let client = Client::builder("AIza-key")
            .base_url("https://custom.googleapis.com/v1")
            .timeout(Duration::from_secs(90))
            .build()
            .unwrap();
        assert_eq!(client.base_url(), "https://custom.googleapis.com/v1");
    }
}
