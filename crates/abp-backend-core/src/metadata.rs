// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backend metadata and rate-limit types.

use serde::{Deserialize, Serialize};

/// Rate-limit configuration for a backend.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateLimit {
    /// Maximum requests per minute.
    pub requests_per_minute: u32,
    /// Maximum tokens per minute.
    pub tokens_per_minute: u64,
    /// Maximum concurrent in-flight requests.
    pub concurrent_requests: u32,
}

/// Descriptive metadata about a backend.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackendMetadata {
    /// Human-readable name.
    pub name: String,
    /// Agent dialect this backend speaks (e.g. `"openai"`, `"anthropic"`).
    pub dialect: String,
    /// Backend version string.
    pub version: String,
    /// Maximum context-window size in tokens, if known.
    pub max_tokens: Option<u64>,
    /// Whether the backend supports streaming responses.
    pub supports_streaming: bool,
    /// Whether the backend supports tool/function calling.
    pub supports_tools: bool,
    /// Optional rate-limit configuration.
    pub rate_limit: Option<RateLimit>,
}
