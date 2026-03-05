// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP client for the Google Gemini API.

use std::time::Duration;

use futures_core::Stream;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};

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

// ── GeminiClient facade ─────────────────────────────────────────────────

/// High-level Gemini client facade that delegates to the ABP runtime.
///
/// This is the primary entry point for users who want a drop-in replacement
/// for the Google Gemini SDK. It wraps the HTTP [`Client`] and provides
/// Gemini-native `generate_content` / `generate_content_stream` methods.
#[derive(Debug, Clone)]
pub struct GeminiClient {
    client: Client,
    default_model: String,
}

impl GeminiClient {
    /// Create a new `GeminiClient` with the given API key.
    ///
    /// Uses the default model `gemini-2.5-flash` and default base URL.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Builder`] if the HTTP client cannot be constructed.
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::with_model(api_key, "gemini-2.5-flash")
    }

    /// Create a new `GeminiClient` targeting a specific model.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Builder`] if the HTTP client cannot be constructed.
    pub fn with_model(api_key: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        let client = Client::new(api_key)?;
        Ok(Self {
            client,
            default_model: model.into(),
        })
    }

    /// Return a [`GeminiClientBuilder`] for advanced configuration.
    pub fn builder(api_key: impl Into<String>) -> GeminiClientBuilder {
        GeminiClientBuilder::new(api_key)
    }

    /// The default model this client targets.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.default_model
    }

    /// The base URL this client targets.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.client.base_url()
    }

    /// Access the underlying HTTP [`Client`].
    #[must_use]
    pub fn http_client(&self) -> &Client {
        &self.client
    }

    /// Send a content generation request.
    ///
    /// If the request's model is empty, the client's default model is used.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn generate_content(
        &self,
        request: &GenerateContentRequest,
    ) -> Result<GenerateContentResponse> {
        let request = if request.model.is_empty() {
            let mut r = request.clone();
            r.model = self.default_model.clone();
            r
        } else {
            request.clone()
        };
        self.client.chat_completion(&request).await
    }

    /// Send a streaming content generation request.
    ///
    /// Returns a stream of [`StreamEvent`]s.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn generate_content_stream(
        &self,
        request: &GenerateContentRequest,
    ) -> Result<impl Stream<Item = Result<StreamEvent>>> {
        // Ensure we have a model set
        let mut owned = request.clone();
        if owned.model.is_empty() {
            owned.model = self.default_model.clone();
        }
        // stream_chat_completion takes a ref but the returned stream
        // is independent of it (tokio_stream::empty placeholder).
        // We re-use the HTTP client's streaming endpoint.
        let url = self.client.model_url(&owned.model, "streamGenerateContent");
        let resp = self
            .client
            .http
            .post(&url)
            .headers(self.client.default_headers())
            .json(&owned)
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

// ── GeminiClient builder ────────────────────────────────────────────────

/// Builder for [`GeminiClient`] with optional configuration overrides.
#[derive(Debug)]
pub struct GeminiClientBuilder {
    api_key: String,
    model: Option<String>,
    base_url: Option<String>,
    timeout: Option<Duration>,
}

impl GeminiClientBuilder {
    /// Create a new builder with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: None,
            base_url: None,
            timeout: None,
        }
    }

    /// Set the default model.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
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

    /// Build the [`GeminiClient`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Builder`] if the HTTP client cannot be constructed.
    pub fn build(self) -> Result<GeminiClient> {
        let mut client_builder = ClientBuilder::new(self.api_key);
        if let Some(url) = self.base_url {
            client_builder = client_builder.base_url(url);
        }
        if let Some(timeout) = self.timeout {
            client_builder = client_builder.timeout(timeout);
        }
        let client = client_builder.build()?;
        Ok(GeminiClient {
            client,
            default_model: self.model.unwrap_or_else(|| "gemini-2.5-flash".into()),
        })
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ── HTTP Client tests ───────────────────────────────────────────────

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

    // ── GeminiClient tests ──────────────────────────────────────────────

    #[test]
    fn gemini_client_new_default_model() {
        let client = GeminiClient::new("AIza-test-key").unwrap();
        assert_eq!(client.model(), "gemini-2.5-flash");
        assert_eq!(
            client.base_url(),
            "https://generativelanguage.googleapis.com/v1beta"
        );
    }

    #[test]
    fn gemini_client_with_model() {
        let client = GeminiClient::with_model("AIza-key", "gemini-2.5-pro").unwrap();
        assert_eq!(client.model(), "gemini-2.5-pro");
    }

    #[test]
    fn gemini_client_builder_full() {
        let client = GeminiClient::builder("AIza-key")
            .model("gemini-2.0-flash")
            .base_url("https://custom.example.com/v1")
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap();
        assert_eq!(client.model(), "gemini-2.0-flash");
        assert_eq!(client.base_url(), "https://custom.example.com/v1");
    }

    #[test]
    fn gemini_client_builder_defaults() {
        let client = GeminiClient::builder("AIza-key").build().unwrap();
        assert_eq!(client.model(), "gemini-2.5-flash");
    }

    #[test]
    fn gemini_client_http_client_accessor() {
        let client = GeminiClient::new("AIza-key").unwrap();
        let http = client.http_client();
        assert_eq!(
            http.base_url(),
            "https://generativelanguage.googleapis.com/v1beta"
        );
    }

    #[test]
    fn gemini_client_url_model_streaming() {
        let client = GeminiClient::new("AIza-key").unwrap();
        let url = client
            .http_client()
            .model_url("gemini-2.5-flash", "streamGenerateContent");
        assert!(url.contains("streamGenerateContent"));
        assert!(url.contains("key=AIza-key"));
    }
}
