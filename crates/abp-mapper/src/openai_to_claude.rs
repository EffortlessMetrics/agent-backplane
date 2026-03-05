// SPDX-License-Identifier: MIT OR Apache-2.0

//! Maps OpenAI chat-completions format to Claude messages API format.

use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use serde_json::{Value, json};

use crate::{DialectRequest, DialectResponse, Mapper, MappingError};

/// Maps OpenAI chat-completions requests/responses to Claude messages API format.
///
/// # Mapping summary
///
/// | OpenAI field | Claude field | Notes |
/// |---|---|---|
/// | `model` | `model` | Passed through (caller should remap model names) |
/// | `messages` | `messages` | Role/content restructured |
/// | `messages[role=system]` | `system` | Extracted to top-level `system` field |
/// | `max_tokens` | `max_tokens` | Direct mapping |
/// | `temperature` | `temperature` | Direct mapping |
/// | `tools` | `tools` | Function schema → Claude tool schema |
/// | `stream` | `stream` | Direct mapping |
///
/// # Examples
///
/// ```
/// use abp_mapper::{Mapper, OpenAiToClaudeMapper, DialectRequest};
/// use abp_dialect::Dialect;
/// use serde_json::json;
///
/// let mapper = OpenAiToClaudeMapper;
/// let req = DialectRequest {
///     dialect: Dialect::OpenAi,
///     body: json!({
///         "model": "gpt-4",
///         "messages": [
///             {"role": "user", "content": "Hello"}
///         ],
///         "max_tokens": 1024
///     }),
/// };
/// let result = mapper.map_request(&req).unwrap();
/// assert_eq!(result["max_tokens"], 1024);
/// assert_eq!(result["messages"][0]["role"], "user");
/// ```
pub struct OpenAiToClaudeMapper;

impl Mapper for OpenAiToClaudeMapper {
    fn map_request(&self, from: &DialectRequest) -> Result<Value, MappingError> {
        if from.dialect != Dialect::OpenAi {
            return Err(MappingError::UnmappableRequest {
                reason: format!(
                    "OpenAiToClaudeMapper expects OpenAI dialect, got {}",
                    from.dialect
                ),
            });
        }

        let obj = from
            .body
            .as_object()
            .ok_or_else(|| MappingError::UnmappableRequest {
                reason: "request body must be a JSON object".into(),
            })?;

        let mut result = serde_json::Map::new();

        // model passthrough
        if let Some(model) = obj.get("model") {
            result.insert("model".into(), model.clone());
        }

        // Extract system messages and convert the rest
        if let Some(Value::Array(messages)) = obj.get("messages") {
            let (system_parts, claude_messages) = split_messages(messages)?;

            if !system_parts.is_empty() {
                result.insert("system".into(), Value::String(system_parts.join("\n\n")));
            }

            result.insert("messages".into(), Value::Array(claude_messages));
        }

        // max_tokens (required by Claude API)
        if let Some(max_tokens) = obj.get("max_tokens") {
            result.insert("max_tokens".into(), max_tokens.clone());
        } else {
            result.insert("max_tokens".into(), json!(4096));
        }

        // temperature
        if let Some(temp) = obj.get("temperature") {
            result.insert("temperature".into(), temp.clone());
        }

        // stream
        if let Some(stream) = obj.get("stream") {
            result.insert("stream".into(), stream.clone());
        }

        // tools: OpenAI function-calling → Claude tool format
        if let Some(Value::Array(tools)) = obj.get("tools") {
            let claude_tools = map_tools_openai_to_claude(tools)?;
            result.insert("tools".into(), Value::Array(claude_tools));
        }

        // stop_sequences (OpenAI "stop" → Claude "stop_sequences")
        if let Some(stop) = obj.get("stop") {
            match stop {
                Value::String(s) => {
                    result.insert("stop_sequences".into(), json!([s]));
                }
                Value::Array(_) => {
                    result.insert("stop_sequences".into(), stop.clone());
                }
                _ => {}
            }
        }

        // top_p
        if let Some(top_p) = obj.get("top_p") {
            result.insert("top_p".into(), top_p.clone());
        }

        Ok(Value::Object(result))
    }

    fn map_response(&self, from: &Value) -> Result<DialectResponse, MappingError> {
        // Claude response → OpenAI-like DialectResponse tagged as Claude
        Ok(DialectResponse {
            dialect: Dialect::Claude,
            body: from.clone(),
        })
    }

    fn map_event(&self, from: &AgentEvent) -> Result<Value, MappingError> {
        // Map ABP AgentEvent to Claude SSE-style event JSON
        match &from.kind {
            AgentEventKind::AssistantDelta { text } => Ok(json!({
                "type": "content_block_delta",
                "delta": {
                    "type": "text_delta",
                    "text": text
                }
            })),
            AgentEventKind::AssistantMessage { text } => Ok(json!({
                "type": "message",
                "content": [{
                    "type": "text",
                    "text": text
                }],
                "role": "assistant"
            })),
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => Ok(json!({
                "type": "content_block_start",
                "content_block": {
                    "type": "tool_use",
                    "id": tool_use_id.as_deref().unwrap_or(""),
                    "name": tool_name,
                    "input": input
                }
            })),
            AgentEventKind::ToolResult {
                tool_use_id,
                output,
                is_error,
                ..
            } => Ok(json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id.as_deref().unwrap_or(""),
                "content": output,
                "is_error": is_error
            })),
            _ => {
                // Fall back to generic serialization for other event kinds
                serde_json::to_value(from).map_err(|e| MappingError::UnmappableRequest {
                    reason: format!("failed to serialize event: {e}"),
                })
            }
        }
    }

    fn source_dialect(&self) -> Dialect {
        Dialect::OpenAi
    }

    fn target_dialect(&self) -> Dialect {
        Dialect::Claude
    }
}

/// Splits OpenAI messages into system text and Claude-formatted messages.
fn split_messages(messages: &[Value]) -> Result<(Vec<String>, Vec<Value>), MappingError> {
    let mut system_parts = Vec::new();
    let mut claude_msgs = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");

        match role {
            "system" => {
                if let Some(content) = msg.get("content").and_then(Value::as_str) {
                    system_parts.push(content.to_owned());
                }
            }
            "user" => {
                claude_msgs.push(map_user_message(msg));
            }
            "assistant" => {
                claude_msgs.push(map_assistant_message(msg)?);
            }
            "tool" => {
                claude_msgs.push(map_tool_result_message(msg));
            }
            other => {
                return Err(MappingError::IncompatibleTypes {
                    source_type: format!("role:{other}"),
                    target_type: "claude_role".into(),
                    reason: format!("unknown OpenAI role `{other}`"),
                });
            }
        }
    }

    Ok((system_parts, claude_msgs))
}

/// Maps an OpenAI user message to Claude format.
fn map_user_message(msg: &Value) -> Value {
    let content = msg.get("content").cloned().unwrap_or(Value::Null);
    match &content {
        // Simple string content
        Value::String(text) => json!({
            "role": "user",
            "content": text
        }),
        // Array content (multimodal) — pass through as-is
        Value::Array(_) => json!({
            "role": "user",
            "content": content
        }),
        _ => json!({
            "role": "user",
            "content": content.to_string()
        }),
    }
}

/// Maps an OpenAI assistant message to Claude format, including tool_calls.
fn map_assistant_message(msg: &Value) -> Result<Value, MappingError> {
    let mut content_blocks: Vec<Value> = Vec::new();

    // Text content
    if let Some(text) = msg.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            content_blocks.push(json!({
                "type": "text",
                "text": text
            }));
        }
    }

    // tool_calls → tool_use content blocks
    if let Some(Value::Array(tool_calls)) = msg.get("tool_calls") {
        for tc in tool_calls {
            let function = tc.get("function").unwrap_or(tc);
            let name = function.get("name").and_then(Value::as_str).unwrap_or("");
            let id = tc.get("id").and_then(Value::as_str).unwrap_or("");

            // Parse arguments string into JSON value
            let input = if let Some(args_str) = function.get("arguments").and_then(Value::as_str) {
                serde_json::from_str(args_str).unwrap_or(Value::Object(serde_json::Map::new()))
            } else {
                function
                    .get("arguments")
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new()))
            };

            content_blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }

    if content_blocks.is_empty() {
        content_blocks.push(json!({"type": "text", "text": ""}));
    }

    Ok(json!({
        "role": "assistant",
        "content": content_blocks
    }))
}

/// Maps an OpenAI tool-result message to Claude tool_result format.
fn map_tool_result_message(msg: &Value) -> Value {
    let tool_call_id = msg
        .get("tool_call_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let content = msg.get("content").cloned().unwrap_or(Value::Null);

    json!({
        "role": "user",
        "content": [{
            "type": "tool_result",
            "tool_use_id": tool_call_id,
            "content": content
        }]
    })
}

/// Maps OpenAI function-calling tools to Claude tool format.
fn map_tools_openai_to_claude(tools: &[Value]) -> Result<Vec<Value>, MappingError> {
    let mut claude_tools = Vec::new();

    for tool in tools {
        let function = tool
            .get("function")
            .ok_or_else(|| MappingError::IncompatibleTypes {
                source_type: "openai_tool".into(),
                target_type: "claude_tool".into(),
                reason: "OpenAI tool missing `function` field".into(),
            })?;

        let name = function.get("name").and_then(Value::as_str).unwrap_or("");
        let description = function
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let parameters = function
            .get("parameters")
            .cloned()
            .unwrap_or(json!({"type": "object", "properties": {}}));

        claude_tools.push(json!({
            "name": name,
            "description": description,
            "input_schema": parameters
        }));
    }

    Ok(claude_tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::AgentEvent;
    use chrono::Utc;

    // ── map_request ─────────────────────────────────────────────────────

    #[test]
    fn basic_user_message() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hello"}
                ],
                "max_tokens": 1024
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["model"], "gpt-4");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"], "Hello");
    }

    #[test]
    fn system_message_extracted() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "system", "content": "You are helpful."},
                    {"role": "user", "content": "Hi"}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["system"], "You are helpful.");
        // Only the user message should remain in messages array
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn multiple_system_messages_joined() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "system", "content": "Rule one."},
                    {"role": "system", "content": "Rule two."},
                    {"role": "user", "content": "Go"}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["system"], "Rule one.\n\nRule two.");
    }

    #[test]
    fn default_max_tokens_when_missing() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["max_tokens"], 4096);
    }

    #[test]
    fn temperature_mapped() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "temperature": 0.7
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["temperature"], 0.7);
    }

    #[test]
    fn stream_flag_mapped() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "stream": true
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["stream"], true);
    }

    #[test]
    fn tools_mapped() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather for a location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {"type": "string"}
                            },
                            "required": ["location"]
                        }
                    }
                }]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["description"], "Get weather for a location");
        assert!(tools[0]["input_schema"]["properties"]["location"].is_object());
    }

    #[test]
    fn stop_string_becomes_array() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "stop": "END"
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["stop_sequences"], json!(["END"]));
    }

    #[test]
    fn stop_array_passed_through() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "stop": ["END", "STOP"]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["stop_sequences"], json!(["END", "STOP"]));
    }

    #[test]
    fn top_p_mapped() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "top_p": 0.9
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["top_p"], 0.9);
    }

    #[test]
    fn assistant_with_tool_calls() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "What's the weather?"},
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"location\":\"NYC\"}"
                            }
                        }]
                    },
                    {
                        "role": "tool",
                        "tool_call_id": "call_123",
                        "content": "72°F, sunny"
                    }
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);

        // Assistant message should have tool_use content block
        let asst_content = messages[1]["content"].as_array().unwrap();
        assert_eq!(asst_content[0]["type"], "tool_use");
        assert_eq!(asst_content[0]["id"], "call_123");
        assert_eq!(asst_content[0]["name"], "get_weather");
        assert_eq!(asst_content[0]["input"]["location"], "NYC");

        // Tool result should be wrapped in user message
        assert_eq!(messages[2]["role"], "user");
        let tool_content = messages[2]["content"].as_array().unwrap();
        assert_eq!(tool_content[0]["type"], "tool_result");
        assert_eq!(tool_content[0]["tool_use_id"], "call_123");
    }

    #[test]
    fn wrong_dialect_rejected() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({"model": "claude-3"}),
        };
        let err = mapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn non_object_body_rejected() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!("not an object"),
        };
        let err = mapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    // ── map_response ────────────────────────────────────────────────────

    #[test]
    fn response_tagged_as_claude() {
        let mapper = OpenAiToClaudeMapper;
        let body = json!({"content": [{"type": "text", "text": "hi"}]});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.dialect, Dialect::Claude);
        assert_eq!(resp.body, body);
    }

    // ── map_event ───────────────────────────────────────────────────────

    #[test]
    fn event_assistant_delta() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "Hi".into() },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "content_block_delta");
        assert_eq!(result["delta"]["text"], "Hi");
    }

    #[test]
    fn event_assistant_message() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Done".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "message");
        assert_eq!(result["content"][0]["text"], "Done");
    }

    #[test]
    fn event_tool_call() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_99".into()),
                parent_tool_use_id: None,
                input: json!({"command": "ls"}),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "content_block_start");
        assert_eq!(result["content_block"]["name"], "bash");
        assert_eq!(result["content_block"]["id"], "tu_99");
    }

    #[test]
    fn event_tool_result() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_99".into()),
                output: json!("file1\nfile2"),
                is_error: false,
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["is_error"], false);
    }

    #[test]
    fn event_warning_fallback() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "slow".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "warning");
    }

    #[test]
    fn source_target_dialects() {
        let mapper = OpenAiToClaudeMapper;
        assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
        assert_eq!(mapper.target_dialect(), Dialect::Claude);
    }
}
