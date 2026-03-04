#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Wiremock-based integration tests for all 6 shim crate HTTP clients.

use std::time::Duration;

use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ═══════════════════════════════════════════════════════════════════════════
// OpenAI Shim
// ═══════════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::client::Client;
    use abp_shim_openai::types::*;

    fn sample_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Hello".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    fn success_json() -> serde_json::Value {
        json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1_700_000_000_u64,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }
        })
    }

    fn make_client(uri: &str, key: &str) -> Client {
        Client::builder(key).base_url(uri).build().unwrap()
    }

    #[tokio::test]
    async fn success_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let resp = client.chat_completion(&sample_request()).await.unwrap();
        assert_eq!(resp.id, "chatcmpl-123");
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello! How can I help?")
        );
    }

    #[tokio::test]
    async fn auth_header_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer sk-secret-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-secret-key");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn content_type_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn correct_endpoint_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let _ = client.chat_completion(&sample_request()).await;
    }

    #[tokio::test]
    async fn request_body_contains_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({"model": "gpt-4o"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn error_400() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string("invalid request body"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_openai::client::ClientError::Api { status, body } => {
                assert_eq!(status, 400);
                assert!(body.contains("invalid request"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_openai::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 401);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_429() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_openai::client::ClientError::Api { status, body } => {
                assert_eq!(status, 429);
                assert!(body.contains("rate limited"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_500() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_openai::client::ClientError::Api { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("internal server error"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("data: [DONE]\n\n"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let req = sample_request();
        let _stream = client.stream_chat_completion(&req).await.unwrap();
    }

    #[tokio::test]
    async fn stream_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(503).set_body_string("service unavailable"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-test");
        let req = sample_request();
        let result = client.stream_chat_completion(&req).await;
        match result {
            Err(abp_shim_openai::client::ClientError::Api { status, .. }) => {
                assert_eq!(status, 503);
            }
            Err(other) => panic!("expected Api error, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn builder_custom_base_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .mount(&server)
            .await;

        let client = Client::builder("sk-test")
            .base_url(server.uri())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        assert_eq!(client.base_url(), server.uri());
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Claude Shim
// ═══════════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::client::Client;
    use abp_shim_claude::types::*;

    fn sample_request() -> MessagesRequest {
        MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hello".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    fn success_json() -> serde_json::Value {
        json!({
            "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello! How can I assist you?"}],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 12,
                "output_tokens": 8
            }
        })
    }

    fn make_client(uri: &str, key: &str) -> Client {
        Client::builder(key).base_url(uri).build().unwrap()
    }

    #[tokio::test]
    async fn success_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let resp = client.chat_completion(&sample_request()).await.unwrap();
        assert_eq!(resp.id, "msg_01XFDUDYJgAACzvnptvVoYEL");
        assert_eq!(resp.role, "assistant");
    }

    #[tokio::test]
    async fn auth_header_x_api_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .and(header("x-api-key", "sk-ant-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-secret");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn anthropic_version_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn custom_anthropic_version() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .and(header("anthropic-version", "2024-01-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::builder("sk-ant-test")
            .base_url(server.uri())
            .anthropic_version("2024-01-01")
            .build()
            .unwrap();
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn content_type_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn correct_endpoint_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let _ = client.chat_completion(&sample_request()).await;
    }

    #[tokio::test]
    async fn request_body_contains_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .and(body_partial_json(
                json!({"model": "claude-sonnet-4-20250514"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn error_400() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_claude::client::ClientError::Api { status, body } => {
                assert_eq!(status, 400);
                assert!(body.contains("bad request"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid api key"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_claude::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 401);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_429() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_claude::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 429);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_500() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_claude::client::ClientError::Api { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("internal error"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_string("event: done\n\n"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let req = sample_request();
        let _stream = client.stream_chat_completion(&req).await.unwrap();
    }

    #[tokio::test]
    async fn stream_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(529).set_body_string("overloaded"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-ant-test");
        let req = sample_request();
        let result = client.stream_chat_completion(&req).await;
        match result {
            Err(abp_shim_claude::client::ClientError::Api { status, .. }) => {
                assert_eq!(status, 529);
            }
            Err(other) => panic!("expected Api error, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Gemini Shim
// ═══════════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_shim_gemini::client::Client;
    use abp_shim_gemini::types::*;

    fn sample_request() -> GenerateContentRequest {
        GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]))
    }

    fn success_json() -> serde_json::Value {
        json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hello! How can I help?"}]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 8,
                "totalTokenCount": 13
            }
        })
    }

    fn make_client(uri: &str, key: &str) -> Client {
        Client::builder(key).base_url(uri).build().unwrap()
    }

    #[tokio::test]
    async fn success_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:generateContent"))
            .and(query_param("key", "AIza-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let resp = client.chat_completion(&sample_request()).await.unwrap();
        assert!(!resp.candidates.is_empty());
        assert_eq!(resp.text(), Some("Hello! How can I help?"));
    }

    #[tokio::test]
    async fn api_key_in_query_param() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:generateContent"))
            .and(query_param("key", "AIza-my-secret-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-my-secret-key");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn content_type_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:generateContent"))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn correct_generate_content_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:generateContent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let _ = client.chat_completion(&sample_request()).await;
    }

    #[tokio::test]
    async fn request_body_contains_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:generateContent"))
            .and(body_partial_json(json!({"model": "gemini-2.5-flash"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn error_400() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:generateContent"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_gemini::client::ClientError::Api { status, body } => {
                assert_eq!(status, 400);
                assert!(body.contains("bad request"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:generateContent"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_gemini::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 401);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_429() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:generateContent"))
            .respond_with(ResponseTemplate::new(429).set_body_string("quota exhausted"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_gemini::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 429);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_500() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:generateContent"))
            .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_gemini::client::ClientError::Api { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("server error"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_uses_different_method() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:streamGenerateContent"))
            .and(query_param("key", "AIza-test"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let req = sample_request();
        let _stream = client.stream_chat_completion(&req).await.unwrap();
    }

    #[tokio::test]
    async fn stream_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/gemini-2.5-flash:streamGenerateContent"))
            .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "AIza-test");
        let req = sample_request();
        let result = client.stream_chat_completion(&req).await;
        match result {
            Err(abp_shim_gemini::client::ClientError::Api { status, .. }) => {
                assert_eq!(status, 503);
            }
            Err(other) => panic!("expected Api error, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Codex Shim
// ═══════════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_codex_sdk::dialect::{CodexInputItem, CodexRequest, CodexResponse};
    use abp_shim_codex::client::Client;

    fn sample_request() -> CodexRequest {
        CodexRequest {
            model: "codex-mini".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "Write hello world".into(),
            }],
            max_output_tokens: Some(1024),
            temperature: None,
            tools: vec![],
            text: None,
        }
    }

    fn success_json() -> serde_json::Value {
        json!({
            "id": "resp-001",
            "model": "codex-mini",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "print('hello world')"}]
            }],
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15
            }
        })
    }

    fn make_client(uri: &str, key: &str) -> Client {
        Client::builder(key).base_url(uri).build().unwrap()
    }

    #[tokio::test]
    async fn success_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let resp: CodexResponse = client.chat_completion(&sample_request()).await.unwrap();
        assert_eq!(resp.id, "resp-001");
        assert_eq!(resp.model, "codex-mini");
    }

    #[tokio::test]
    async fn auth_header_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .and(header("authorization", "Bearer sk-codex-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-secret");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn content_type_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn correct_endpoint_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let _ = client.chat_completion(&sample_request()).await;
    }

    #[tokio::test]
    async fn request_body_contains_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .and(body_partial_json(json!({"model": "codex-mini"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn error_400() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_codex::client::ClientError::Api { status, body } => {
                assert_eq!(status, 400);
                assert!(body.contains("bad request"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_codex::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 401);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_429() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_codex::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 429);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_500() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_codex::client::ClientError::Api { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("internal error"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_string("data: [DONE]\n\n"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let req = sample_request();
        let _stream = client.stream_chat_completion(&req).await.unwrap();
    }

    #[tokio::test]
    async fn stream_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(502).set_body_string("bad gateway"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-codex-test");
        let req = sample_request();
        let result = client.stream_chat_completion(&req).await;
        match result {
            Err(abp_shim_codex::client::ClientError::Api { status, .. }) => {
                assert_eq!(status, 502);
            }
            Err(other) => panic!("expected Api error, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Copilot Shim
// ═══════════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_copilot_sdk::dialect::{CopilotMessage, CopilotRequest, CopilotResponse};
    use abp_shim_copilot::client::Client;

    fn sample_request() -> CopilotRequest {
        CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "Hello".into(),
                name: None,
                copilot_references: vec![],
            }],
            tools: None,
            turn_history: vec![],
            references: vec![],
        }
    }

    fn success_json() -> serde_json::Value {
        json!({
            "message": "Hello! How can I help you today?",
            "copilot_references": [],
            "copilot_errors": []
        })
    }

    fn make_client(uri: &str, key: &str) -> Client {
        Client::builder(key).base_url(uri).build().unwrap()
    }

    #[tokio::test]
    async fn success_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test-token");
        let resp: CopilotResponse = client.chat_completion(&sample_request()).await.unwrap();
        assert_eq!(resp.message, "Hello! How can I help you today?");
    }

    #[tokio::test]
    async fn auth_header_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer ghu_secret-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_secret-token");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn copilot_integration_id_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("copilot-integration-id", "agent-backplane"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn content_type_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn correct_endpoint_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let _ = client.chat_completion(&sample_request()).await;
    }

    #[tokio::test]
    async fn request_body_contains_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({"model": "gpt-4o"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn error_400() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_copilot::client::ClientError::Api { status, body } => {
                assert_eq!(status, 400);
                assert!(body.contains("bad request"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_copilot::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 401);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_429() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_copilot::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 429);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_500() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_copilot::client::ClientError::Api { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("server error"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("data: [DONE]\n\n"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let req = sample_request();
        let _stream = client.stream_chat_completion(&req).await.unwrap();
    }

    #[tokio::test]
    async fn stream_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "ghu_test");
        let req = sample_request();
        let result = client.stream_chat_completion(&req).await;
        match result {
            Err(abp_shim_copilot::client::ClientError::Api { status, .. }) => {
                assert_eq!(status, 503);
            }
            Err(other) => panic!("expected Api error, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Kimi Shim
// ═══════════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_kimi_sdk::dialect::{KimiMessage, KimiRequest, KimiResponse};
    use abp_shim_kimi::client::Client;

    fn sample_request() -> KimiRequest {
        KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![KimiMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(1024),
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        }
    }

    fn success_json() -> serde_json::Value {
        json!({
            "id": "cmpl-kimi-001",
            "model": "moonshot-v1-8k",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! I'm Kimi."
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 8,
                "completion_tokens": 6,
                "total_tokens": 14
            }
        })
    }

    fn make_client(uri: &str, key: &str) -> Client {
        Client::builder(key).base_url(uri).build().unwrap()
    }

    #[tokio::test]
    async fn success_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let resp: KimiResponse = client.chat_completion(&sample_request()).await.unwrap();
        assert_eq!(resp.id, "cmpl-kimi-001");
        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello! I'm Kimi.")
        );
    }

    #[tokio::test]
    async fn auth_header_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer sk-kimi-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-secret");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn content_type_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn correct_endpoint_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let _ = client.chat_completion(&sample_request()).await;
    }

    #[tokio::test]
    async fn request_body_contains_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({"model": "moonshot-v1-8k"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_json()))
            .expect(1)
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let resp = client.chat_completion(&sample_request()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn error_400() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_kimi::client::ClientError::Api { status, body } => {
                assert_eq!(status, 400);
                assert!(body.contains("bad request"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_kimi::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 401);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_429() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_kimi::client::ClientError::Api { status, .. } => {
                assert_eq!(status, 429);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_500() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let err = client.chat_completion(&sample_request()).await.unwrap_err();
        match err {
            abp_shim_kimi::client::ClientError::Api { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("server error"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("data: [DONE]\n\n"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let req = sample_request();
        let _stream = client.stream_chat_completion(&req).await.unwrap();
    }

    #[tokio::test]
    async fn stream_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(502).set_body_string("bad gateway"))
            .mount(&server)
            .await;

        let client = make_client(&server.uri(), "sk-kimi-test");
        let req = sample_request();
        let result = client.stream_chat_completion(&req).await;
        match result {
            Err(abp_shim_kimi::client::ClientError::Api { status, .. }) => {
                assert_eq!(status, 502);
            }
            Err(other) => panic!("expected Api error, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: timeout behaviour
// ═══════════════════════════════════════════════════════════════════════════

mod timeout {
    use super::*;
    use abp_shim_openai::client::Client as OpenAiClient;
    use abp_shim_openai::types::*;

    fn sample_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Hello".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    #[tokio::test]
    async fn short_timeout_causes_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("{}")
                    .set_delay(Duration::from_secs(5)),
            )
            .mount(&server)
            .await;

        let client = OpenAiClient::builder("sk-test")
            .base_url(server.uri())
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();

        let result = client.chat_completion(&sample_request()).await;
        assert!(result.is_err());
    }
}
