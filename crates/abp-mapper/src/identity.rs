// SPDX-License-Identifier: MIT OR Apache-2.0

//! Identity (passthrough) mapper — returns inputs unchanged.

use abp_core::AgentEvent;
use abp_dialect::Dialect;

use crate::{DialectRequest, DialectResponse, Mapper, MappingError};

/// A no-op mapper that passes requests, responses, and events through unchanged.
///
/// Useful for same-dialect routing and as a baseline for testing.
///
/// # Examples
///
/// ```
/// use abp_mapper::{Mapper, IdentityMapper, DialectRequest};
/// use abp_dialect::Dialect;
/// use serde_json::json;
///
/// let mapper = IdentityMapper;
/// let req = DialectRequest {
///     dialect: Dialect::OpenAi,
///     body: json!({"model": "gpt-4"}),
/// };
/// let result = mapper.map_request(&req).unwrap();
/// assert_eq!(result, json!({"model": "gpt-4"}));
/// ```
pub struct IdentityMapper;

impl Mapper for IdentityMapper {
    fn map_request(&self, from: &DialectRequest) -> Result<serde_json::Value, MappingError> {
        Ok(from.body.clone())
    }

    fn map_response(&self, from: &serde_json::Value) -> Result<DialectResponse, MappingError> {
        Ok(DialectResponse {
            dialect: Dialect::OpenAi, // passthrough preserves whatever is given
            body: from.clone(),
        })
    }

    fn map_event(&self, from: &AgentEvent) -> Result<serde_json::Value, MappingError> {
        serde_json::to_value(from).map_err(|e| MappingError::UnmappableRequest {
            reason: format!("failed to serialize event: {e}"),
        })
    }

    fn source_dialect(&self) -> Dialect {
        Dialect::OpenAi
    }

    fn target_dialect(&self) -> Dialect {
        Dialect::OpenAi
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{AgentEvent, AgentEventKind};
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn identity_map_request_passthrough() {
        let mapper = IdentityMapper;
        let body = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: body.clone(),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn identity_map_response_passthrough() {
        let mapper = IdentityMapper;
        let body = json!({"id": "chatcmpl-123", "choices": []});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.body, body);
    }

    #[test]
    fn identity_map_event_passthrough() {
        let mapper = IdentityMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert!(result.get("type").is_some());
        assert_eq!(result["text"], "hello");
    }

    #[test]
    fn identity_map_request_empty_object() {
        let mapper = IdentityMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({}),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result, json!({}));
    }

    #[test]
    fn identity_map_event_tool_call() {
        let mapper = IdentityMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["tool_name"], "read_file");
    }

    #[test]
    fn identity_source_target_dialect() {
        let mapper = IdentityMapper;
        assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
        assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
    }

    #[test]
    fn identity_map_request_with_any_dialect() {
        let mapper = IdentityMapper;
        for &d in Dialect::all() {
            let req = DialectRequest {
                dialect: d,
                body: json!({"test": true}),
            };
            let result = mapper.map_request(&req).unwrap();
            assert_eq!(result, json!({"test": true}));
        }
    }

    #[test]
    fn identity_map_response_preserves_nested() {
        let mapper = IdentityMapper;
        let body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello!",
                    "tool_calls": []
                }
            }]
        });
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.body["choices"][0]["message"]["content"], "Hello!");
    }
}
