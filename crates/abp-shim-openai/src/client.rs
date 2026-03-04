// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP client for the OpenAI Chat Completions API.

use std::time::Duration;

use futures_core::Stream;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ErrorResponse, StreamChunk};

// ── Error type ──────────────────────────────────────────────────────────

/// Errors from the HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// HTTP transport error.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// Non-success status code from the API with structured error info.
    #[error("api error (status {status}): {body}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Response body.
        body: String,
        /// Parsed error response, if the body matched the OpenAI error format.
        parsed: Option<ErrorResponse>,
    },
    /// Failed to build the client.
    #[error("builder error: {0}")]
    Builder(String),
    /// SSE stream parse error.
    #[error("stream parse error: {0}")]
    StreamParse(String),
}

impl ClientError {
    /// Try to extract the structured [`ErrorResponse`] from an API error.
    #[must_use]
    pub fn error_response(&self) -> Option<&ErrorResponse> {
        match self {
            Self::Api { parsed, .. } => parsed.as_ref(),
            _ => None,
        }
    }

    /// Return `true` if this is a rate-limit error (HTTP 429).
    #[must_use]
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, Self::Api { status: 429, .. })
    }

    /// Return `true` if this is an authentication error (HTTP 401).
    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        matches!(self, Self::Api { status: 401, .. })
    }
}

/// Result alias for client operations.
pub type Result<T> = std::result::Result<T, ClientError>;

// ── Client ──────────────────────────────────────────────────────────────

/// HTTP client for the OpenAI Chat Completions API.
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

    /// Send a chat completion request.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
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
            let parsed = serde_json::from_str::<ErrorResponse>(&body).ok();
            return Err(ClientError::Api {
                status: status.as_u16(),
                body,
                parsed,
            });
        }
        Ok(resp.json().await?)
    }

    /// Send a streaming chat completion request.
    ///
    /// Returns a stream of [`StreamChunk`]s parsed from the SSE response.
    /// Each SSE `data:` line is parsed as a JSON [`StreamChunk`]. The stream
    /// ends when a `data: [DONE]` sentinel is received.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or API errors.
    pub async fn stream_chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<impl Stream<Item = Result<StreamChunk>>> {
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
            let parsed = serde_json::from_str::<ErrorResponse>(&body).ok();
            return Err(ClientError::Api {
                status: status.as_u16(),
                body,
                parsed,
            });
        }

        // Read the streaming response and parse SSE lines.
        let text = resp.text().await.map_err(ClientError::Http)?;
        let chunks = parse_sse_text(&text);
        let items: Vec<Result<StreamChunk>> = chunks
            .into_iter()
            .map(|r| r.map_err(ClientError::StreamParse))
            .collect();
        Ok(tokio_stream::iter(items))
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
            base_url: self
                .base_url
                .unwrap_or_else(|| "https://api.openai.com/v1".into()),
            api_key: self.api_key,
        })
    }
}

// ── SSE stream parsing ─────────────────────────────────────────────────

/// SSE line-based stream parser.
///
/// Wraps a text buffer and incrementally parses SSE `data:` lines.
pub struct SseLineStream {
    buffer: String,
    done: bool,
    chunks: std::collections::VecDeque<Result<StreamChunk>>,
}

impl SseLineStream {
    /// Create a new SSE line parser.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            done: false,
            chunks: std::collections::VecDeque::new(),
        }
    }

    /// Feed raw bytes into the parser. Call [`drain`](Self::drain) to retrieve parsed chunks.
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.push_str(&String::from_utf8_lossy(data));
        self.parse_lines();
    }

    /// Feed a string into the parser.
    pub fn feed_str(&mut self, data: &str) {
        self.buffer.push_str(data);
        self.parse_lines();
    }

    /// Signal end of input. Parses any remaining data in the buffer.
    pub fn finish(&mut self) {
        self.done = true;
        // Parse any trailing data
        let remaining = self.buffer.trim().to_string();
        self.buffer.clear();
        if !remaining.is_empty() {
            if let Some(data) = remaining.strip_prefix("data: ") {
                let data = data.trim();
                if data != "[DONE]" {
                    if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                        self.chunks.push_back(Ok(chunk));
                    }
                }
            }
        }
    }

    /// Drain all parsed chunks.
    pub fn drain(&mut self) -> impl Iterator<Item = Result<StreamChunk>> + '_ {
        self.chunks.drain(..)
    }

    /// Return `true` if the `[DONE]` sentinel has been received.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.done
    }

    fn parse_lines(&mut self) {
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos]
                .trim_end_matches('\r')
                .to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                let data = data.trim();
                if data == "[DONE]" {
                    self.done = true;
                    return;
                }
                match serde_json::from_str::<StreamChunk>(data) {
                    Ok(chunk) => self.chunks.push_back(Ok(chunk)),
                    Err(e) => self.chunks.push_back(Err(ClientError::StreamParse(format!(
                        "failed to parse SSE chunk: {e}"
                    )))),
                }
            }
            // Skip non-data lines (comments, event types, etc.)
        }
    }
}

impl Default for SseLineStream {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a single SSE data line into a [`StreamChunk`].
///
/// Returns `None` for the `[DONE]` sentinel and non-data lines.
pub fn parse_sse_line(line: &str) -> Option<std::result::Result<StreamChunk, String>> {
    let data = line.strip_prefix("data: ")?.trim();
    if data == "[DONE]" {
        return None;
    }
    Some(serde_json::from_str(data).map_err(|e| format!("SSE parse error: {e}")))
}

/// Parse multiple SSE lines from a complete text block.
///
/// Useful for testing or processing buffered SSE data.
pub fn parse_sse_text(text: &str) -> Vec<std::result::Result<StreamChunk, String>> {
    let mut parser = SseLineStream::new();
    parser.feed_str(text);
    parser.finish();
    parser
        .drain()
        .map(|r| r.map_err(|e| e.to_string()))
        .collect()
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
        let url = format!("{}/chat/completions", client.base_url());
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn authorization_header() {
        let client = Client::new("sk-abc123").unwrap();
        let headers = client.default_headers();
        let auth = headers.get(AUTHORIZATION).unwrap().to_str().unwrap();
        assert_eq!(auth, "Bearer sk-abc123");
        assert_eq!(
            headers.get(CONTENT_TYPE).unwrap().to_str().unwrap(),
            "application/json"
        );
    }

    #[test]
    fn error_display() {
        let err = ClientError::Api {
            status: 429,
            body: "rate limited".into(),
            parsed: None,
        };
        let msg = err.to_string();
        assert!(msg.contains("429"));
        assert!(msg.contains("rate limited"));
    }

    #[test]
    fn error_is_rate_limit() {
        let err = ClientError::Api {
            status: 429,
            body: "rate limited".into(),
            parsed: None,
        };
        assert!(err.is_rate_limit());
        assert!(!err.is_auth_error());
    }

    #[test]
    fn error_is_auth_error() {
        let err = ClientError::Api {
            status: 401,
            body: "unauthorized".into(),
            parsed: None,
        };
        assert!(err.is_auth_error());
        assert!(!err.is_rate_limit());
    }

    #[test]
    fn error_with_parsed_response() {
        let parsed = ErrorResponse::rate_limit("Too many requests");
        let err = ClientError::Api {
            status: 429,
            body: serde_json::to_string(&parsed).unwrap(),
            parsed: Some(parsed.clone()),
        };
        let resp = err.error_response().unwrap();
        assert_eq!(resp.error.error_type, "rate_limit_error");
        assert!(resp.error.message.contains("Too many"));
    }

    #[test]
    fn parse_sse_line_data() {
        let chunk_json = r#"{"id":"chatcmpl-1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}]}"#;
        let line = format!("data: {chunk_json}");
        let result = parse_sse_line(&line).unwrap().unwrap();
        assert_eq!(result.id, "chatcmpl-1");
        assert_eq!(result.choices[0].delta.content.as_deref(), Some("Hi"));
    }

    #[test]
    fn parse_sse_line_done() {
        assert!(parse_sse_line("data: [DONE]").is_none());
    }

    #[test]
    fn parse_sse_line_non_data() {
        assert!(parse_sse_line("event: message").is_none());
        assert!(parse_sse_line(": comment").is_none());
        assert!(parse_sse_line("").is_none());
    }

    #[test]
    fn stream_parse_error_display() {
        let err = ClientError::StreamParse("bad json".into());
        assert!(err.to_string().contains("bad json"));
    }

    #[test]
    fn builder_base_url_override() {
        let client = Client::builder("sk-key")
            .base_url("https://custom.example.com/v1")
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        assert_eq!(client.base_url(), "https://custom.example.com/v1");
    }

    // ── SseLineStream tests ─────────────────────────────────────────────

    #[test]
    fn sse_line_stream_parses_single_chunk() {
        let chunk_json = r#"{"id":"chatcmpl-1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}]}"#;
        let sse_data = format!("data: {chunk_json}\n\n");

        let mut parser = SseLineStream::new();
        parser.feed_str(&sse_data);
        let chunks: Vec<_> = parser.drain().collect();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_ok());
        assert_eq!(chunks[0].as_ref().unwrap().id, "chatcmpl-1");
    }

    #[test]
    fn sse_line_stream_parses_multiple_chunks() {
        let chunk1 = r#"{"id":"c1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#;
        let chunk2 = r#"{"id":"c1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}]}"#;
        let sse_data = format!("data: {chunk1}\n\ndata: {chunk2}\n\ndata: [DONE]\n\n");

        let mut parser = SseLineStream::new();
        parser.feed_str(&sse_data);
        assert!(parser.is_done());
        let chunks: Vec<_> = parser.drain().collect();
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn sse_line_stream_stops_at_done() {
        let chunk = r#"{"id":"c1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}]}"#;
        let sse_data = format!("data: {chunk}\n\ndata: [DONE]\n\n");

        let mut parser = SseLineStream::new();
        parser.feed_str(&sse_data);
        assert!(parser.is_done());
        let chunks: Vec<_> = parser.drain().collect();
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn sse_line_stream_incremental_feed() {
        let chunk_json = r#"{"id":"c1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}]}"#;

        let mut parser = SseLineStream::new();
        // Feed data in small increments
        parser.feed_str("data: ");
        assert_eq!(parser.drain().count(), 0); // Not enough data yet
        parser.feed_str(chunk_json);
        assert_eq!(parser.drain().count(), 0); // Still no newline
        parser.feed_str("\n\n");
        let chunks: Vec<_> = parser.drain().collect();
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn sse_line_stream_skips_comments() {
        let chunk_json = r#"{"id":"c1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}]}"#;
        let sse_data = format!(": this is a comment\nevent: message\ndata: {chunk_json}\n\n");

        let mut parser = SseLineStream::new();
        parser.feed_str(&sse_data);
        let chunks: Vec<_> = parser.drain().collect();
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn sse_line_stream_invalid_json_returns_error() {
        let sse_data = "data: {invalid json}\n\n";
        let mut parser = SseLineStream::new();
        parser.feed_str(sse_data);
        let chunks: Vec<_> = parser.drain().collect();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_err());
    }

    #[test]
    fn parse_sse_text_full_conversation() {
        let chunk1 = r#"{"id":"c1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"role":"assistant","content":"He"},"finish_reason":null}]}"#;
        let chunk2 = r#"{"id":"c1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"content":"llo"},"finish_reason":null}]}"#;
        let chunk3 = r#"{"id":"c1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;

        let sse_text =
            format!("data: {chunk1}\n\ndata: {chunk2}\n\ndata: {chunk3}\n\ndata: [DONE]\n\n");

        let results = parse_sse_text(&sse_text);
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_ok()));

        let c1 = results[0].as_ref().unwrap();
        assert_eq!(c1.choices[0].delta.role.as_deref(), Some("assistant"));
        assert_eq!(c1.choices[0].delta.content.as_deref(), Some("He"));

        let c2 = results[1].as_ref().unwrap();
        assert_eq!(c2.choices[0].delta.content.as_deref(), Some("llo"));

        let c3 = results[2].as_ref().unwrap();
        assert_eq!(c3.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn sse_line_stream_with_tool_call_chunk() {
        let chunk_json = r#"{"id":"c1","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"main.rs\"}"}}]},"finish_reason":null}]}"#;
        let sse_data = format!("data: {chunk_json}\n\n");

        let mut parser = SseLineStream::new();
        parser.feed_str(&sse_data);
        let chunks: Vec<_> = parser.drain().collect();
        assert_eq!(chunks.len(), 1);
        let tc = &chunks[0].as_ref().unwrap().choices[0]
            .delta
            .tool_calls
            .as_ref()
            .unwrap()[0];
        assert_eq!(tc.id.as_deref(), Some("call_1"));
        assert_eq!(
            tc.function.as_ref().unwrap().name.as_deref(),
            Some("read_file")
        );
    }
}
