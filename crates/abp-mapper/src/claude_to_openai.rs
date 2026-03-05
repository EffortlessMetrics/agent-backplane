// SPDX-License-Identifier: MIT OR Apache-2.0

//! Maps Claude messages API format to OpenAI chat-completions format.

use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use serde_json::{Value, json};

use crate::{DialectRequest, DialectResponse, Mapper, MappingError};

/// Maps Claude messages API requests/responses to OpenAI chat-completions format.
///
/// # Mapping summary
///
/// | Claude field | OpenAI field | Notes |
/// |---|---|---|
/// | `model` | `model` | Passed through (caller should remap model names) |
/// | `system` | `messages[0]{role:system}` | Prepended as system message |
/// | `messages` | `messages` | Role/content restructured |
/// | `max_tokens` | `max_tokens` | Direct mapping |
/// | `temperature` | `temperature` | Direct mapping |
/// | `tools` | `tools` | Claude tool schema → OpenAI function schema |
/// | `stream` | `stream` | Direct mapping |
///
/// # Examples
///
/// ```
/// use abp_mapper::{Mapper, ClaudeToOpenAiMapper, DialectRequest};
/// use abp_dialect::Dialect;
/// use serde_json::json;
///
/// let mapper = ClaudeToOpenAiMapper;
/// let req = DialectRequest {
///     dialect: Dialect::Claude,
///     body: json!({
///         "model": "claude-3-5-sonnet-20241022",
///         "max_tokens": 1024,
///         "messages": [
///             {"role": "user", "content": "Hello"}
///         ]
///     }),
/// };
/// let result = mapper.map_request(&req).unwrap();
/// assert_eq!(result["max_tokens"], 1024);
/// assert_eq!(result["messages"][0]["role"], "user");
/// assert_eq!(result["messages"][0]["content"], "Hello");
/// ```
pub struct ClaudeToOpenAiMapper;

impl Mapper for ClaudeToOpenAiMapper {
    fn map_request(&self, from: &DialectRequest) -> Result<Value, MappingError> {
        if from.dialect != Dialect::Claude {
            return Err(MappingError::UnmappableRequest {
                reason: format!(
                    "ClaudeToOpenAiMapper expects Claude dialect, got {}",
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

        let mut openai_messages: Vec<Value> = Vec::new();

        // system → system message
        if let Some(system) = obj.get("system") {
            match system {
                Value::String(s) => {
                    openai_messages.push(json!({"role": "system", "content": s}));
                }
                // Claude also supports array-of-blocks for system
                Value::Array(blocks) => {
                    let text: Vec<String> = blocks
                        .iter()
                        .filter_map(|b| b.get("text").and_then(Value::as_str).map(String::from))
                        .collect();
                    if !text.is_empty() {
                        openai_messages
                            .push(json!({"role": "system", "content": text.join("\n\n")}));
                    }
                }
                _ => {}
            }
        }

        // Convert Claude messages
        if let Some(Value::Array(messages)) = obj.get("messages") {
            for msg in messages {
                let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
                match role {
                    "user" => {
                        openai_messages.push(map_claude_user_message(msg));
                    }
                    "assistant" => {
                        let mapped = map_claude_assistant_message(msg)?;
                        openai_messages.extend(mapped);
                    }
                    other => {
                        return Err(MappingError::IncompatibleTypes {
                            source_type: format!("role:{other}"),
                            target_type: "openai_role".into(),
                            reason: format!("unknown Claude role `{other}`"),
                        });
                    }
                }
            }
        }

        result.insert("messages".into(), Value::Array(openai_messages));

        // max_tokens
        if let Some(max_tokens) = obj.get("max_tokens") {
            result.insert("max_tokens".into(), max_tokens.clone());
        }

        // temperature
        if let Some(temp) = obj.get("temperature") {
            result.insert("temperature".into(), temp.clone());
        }

        // stream
        if let Some(stream) = obj.get("stream") {
            result.insert("stream".into(), stream.clone());
        }

        // tools: Claude tool format → OpenAI function-calling
        if let Some(Value::Array(tools)) = obj.get("tools") {
            let openai_tools = map_tools_claude_to_openai(tools)?;
            result.insert("tools".into(), Value::Array(openai_tools));
        }

        // stop_sequences → stop
        if let Some(stop) = obj.get("stop_sequences") {
            result.insert("stop".into(), stop.clone());
        }

        // top_p
        if let Some(top_p) = obj.get("top_p") {
            result.insert("top_p".into(), top_p.clone());
        }

        Ok(Value::Object(result))
    }

    fn map_response(&self, from: &Value) -> Result<DialectResponse, MappingError> {
        Ok(DialectResponse {
            dialect: Dialect::OpenAi,
            body: from.clone(),
        })
    }

    fn map_event(&self, from: &AgentEvent) -> Result<Value, MappingError> {
        // Map ABP AgentEvent to OpenAI SSE-style event JSON
        match &from.kind {
            AgentEventKind::AssistantDelta { text } => Ok(json!({
                "object": "chat.completion.chunk",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": text
                    }
                }]
            })),
            AgentEventKind::AssistantMessage { text } => Ok(json!({
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": text
                    },
                    "finish_reason": "stop"
                }]
            })),
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                let args = serde_json::to_string(input).unwrap_or_else(|_| "{}".into());
                Ok(json!({
                    "object": "chat.completion.chunk",
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "id": tool_use_id.as_deref().unwrap_or(""),
                                "type": "function",
                                "function": {
                                    "name": tool_name,
                                    "arguments": args
                                }
                            }]
                        }
                    }]
                }))
            }
            AgentEventKind::ToolResult {
                tool_use_id,
                output,
                ..
            } => {
                let content = match output {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                Ok(json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id.as_deref().unwrap_or(""),
                    "content": content
                }))
            }
            _ => {
                // Fall back to generic serialization
                serde_json::to_value(from).map_err(|e| MappingError::UnmappableRequest {
                    reason: format!("failed to serialize event: {e}"),
                })
            }
        }
    }

    fn source_dialect(&self) -> Dialect {
        Dialect::Claude
    }

    fn target_dialect(&self) -> Dialect {
        Dialect::OpenAi
    }
}

/// Maps a Claude user message to OpenAI format.
///
/// Claude user messages may contain `tool_result` blocks; those are extracted
/// into separate OpenAI `tool` messages. Image blocks are mapped to OpenAI
/// `image_url` content parts.
fn map_claude_user_message(msg: &Value) -> Value {
    let content = msg.get("content").cloned().unwrap_or(Value::Null);

    match &content {
        Value::String(s) => json!({"role": "user", "content": s}),
        Value::Array(blocks) => {
            // Check if it contains tool_result blocks
            let has_tool_results = blocks
                .iter()
                .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"));

            if has_tool_results && blocks.len() == 1 {
                // Single tool_result → OpenAI tool message
                let block = &blocks[0];
                let tool_use_id = block
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let content_val = block.get("content").cloned().unwrap_or(Value::Null);
                let content_str = match &content_val {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": content_str
                })
            } else {
                // Check for image blocks — use multimodal content format
                let has_images = blocks
                    .iter()
                    .any(|b| b.get("type").and_then(Value::as_str) == Some("image"));
                if has_images {
                    let parts: Vec<Value> = blocks
                        .iter()
                        .filter_map(|b| {
                            let btype = b.get("type").and_then(Value::as_str).unwrap_or("");
                            match btype {
                                "text" => {
                                    let text = b.get("text").and_then(Value::as_str).unwrap_or("");
                                    Some(json!({"type": "text", "text": text}))
                                }
                                "image" => {
                                    let source = b.get("source")?;
                                    let media_type = source
                                        .get("media_type")
                                        .and_then(Value::as_str)
                                        .unwrap_or("image/png");
                                    let data =
                                        source.get("data").and_then(Value::as_str).unwrap_or("");
                                    let url = format!("data:{media_type};base64,{data}");
                                    Some(json!({"type": "image_url", "image_url": {"url": url}}))
                                }
                                _ => None,
                            }
                        })
                        .collect();
                    json!({"role": "user", "content": parts})
                } else {
                    // Text-only blocks → extract text
                    let text: Vec<String> = blocks
                        .iter()
                        .filter_map(|b| {
                            if b.get("type").and_then(Value::as_str) == Some("text") {
                                b.get("text").and_then(Value::as_str).map(String::from)
                            } else {
                                None
                            }
                        })
                        .collect();
                    json!({"role": "user", "content": text.join("\n")})
                }
            }
        }
        _ => json!({"role": "user", "content": content}),
    }
}

/// Maps a Claude assistant message to OpenAI format.
///
/// Returns a Vec because a Claude assistant message with tool_use blocks
/// becomes a single OpenAI assistant message with `tool_calls`.
fn map_claude_assistant_message(msg: &Value) -> Result<Vec<Value>, MappingError> {
    let content = msg.get("content").cloned().unwrap_or(Value::Null);

    match &content {
        Value::String(s) => Ok(vec![json!({
            "role": "assistant",
            "content": s
        })]),
        Value::Array(blocks) => {
            let mut text_parts: Vec<String> = Vec::new();
            let mut tool_calls: Vec<Value> = Vec::new();

            for block in blocks {
                let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            text_parts.push(t.to_owned());
                        }
                    }
                    "tool_use" => {
                        let id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let input = block
                            .get("input")
                            .cloned()
                            .unwrap_or(Value::Object(serde_json::Map::new()));
                        let args = serde_json::to_string(&input).unwrap_or_else(|_| "{}".into());

                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": args
                            }
                        }));
                    }
                    _ => {
                        // Skip unknown block types
                    }
                }
            }

            let text_content = if text_parts.is_empty() {
                Value::Null
            } else {
                Value::String(text_parts.join("\n"))
            };

            let mut asst = json!({
                "role": "assistant",
                "content": text_content
            });

            if !tool_calls.is_empty() {
                asst["tool_calls"] = Value::Array(tool_calls);
            }

            Ok(vec![asst])
        }
        _ => Ok(vec![json!({
            "role": "assistant",
            "content": content
        })]),
    }
}

/// Maps Claude tool definitions to OpenAI function-calling format.
fn map_tools_claude_to_openai(tools: &[Value]) -> Result<Vec<Value>, MappingError> {
    let mut openai_tools = Vec::new();

    for tool in tools {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let parameters = tool
            .get("input_schema")
            .cloned()
            .unwrap_or(json!({"type": "object", "properties": {}}));

        openai_tools.push(json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": parameters
            }
        }));
    }

    Ok(openai_tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::AgentEvent;
    use chrono::Utc;

    // ── map_request ─────────────────────────────────────────────────────

    #[test]
    fn basic_user_message() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [
                    {"role": "user", "content": "Hello"}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["model"], "claude-3-5-sonnet-20241022");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"], "Hello");
    }

    #[test]
    fn system_string_becomes_message() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "system": "You are helpful.",
                "messages": [
                    {"role": "user", "content": "Hi"}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are helpful.");
        assert_eq!(messages[1]["role"], "user");
    }

    #[test]
    fn system_array_blocks() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "system": [
                    {"type": "text", "text": "Rule one."},
                    {"type": "text", "text": "Rule two."}
                ],
                "messages": [{"role": "user", "content": "Go"}]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "Rule one.\n\nRule two.");
    }

    #[test]
    fn assistant_with_tool_use() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [
                    {"role": "user", "content": "Weather?"},
                    {
                        "role": "assistant",
                        "content": [
                            {"type": "text", "text": "Let me check."},
                            {
                                "type": "tool_use",
                                "id": "tu_abc",
                                "name": "get_weather",
                                "input": {"city": "NYC"}
                            }
                        ]
                    },
                    {
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": "tu_abc",
                            "content": "72°F"
                        }]
                    }
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // User message
        assert_eq!(messages[0]["role"], "user");

        // Assistant with tool_calls
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "Let me check.");
        let tool_calls = messages[1]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls[0]["id"], "tu_abc");
        assert_eq!(tool_calls[0]["function"]["name"], "get_weather");

        // Tool result → OpenAI tool message
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "tu_abc");
        assert_eq!(messages[2]["content"], "72°F");
    }

    #[test]
    fn tools_mapped() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{
                    "name": "get_weather",
                    "description": "Get weather",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "city": {"type": "string"}
                        }
                    }
                }]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert!(tools[0]["function"]["parameters"]["properties"]["city"].is_object());
    }

    #[test]
    fn stop_sequences_mapped() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "hi"}],
                "stop_sequences": ["END"]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["stop"], json!(["END"]));
    }

    #[test]
    fn temperature_and_top_p() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "hi"}],
                "temperature": 0.5,
                "top_p": 0.8
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["temperature"], 0.5);
        assert_eq!(result["top_p"], 0.8);
    }

    #[test]
    fn wrong_dialect_rejected() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({"model": "gpt-4"}),
        };
        let err = mapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn non_object_body_rejected() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!(42),
        };
        let err = mapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    // ── map_response ────────────────────────────────────────────────────

    #[test]
    fn response_tagged_as_openai() {
        let mapper = ClaudeToOpenAiMapper;
        let body = json!({"choices": [{"message": {"content": "hi"}}]});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.dialect, Dialect::OpenAi);
        assert_eq!(resp.body, body);
    }

    // ── map_event ───────────────────────────────────────────────────────

    #[test]
    fn event_assistant_delta() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "tok".into() },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["object"], "chat.completion.chunk");
        assert_eq!(result["choices"][0]["delta"]["content"], "tok");
    }

    #[test]
    fn event_assistant_message() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Done".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["choices"][0]["message"]["content"], "Done");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn event_tool_call() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "x.rs"}),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["object"], "chat.completion.chunk");
        let tc = &result["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(tc["function"]["name"], "read_file");
        assert_eq!(tc["id"], "call_1");
    }

    #[test]
    fn event_tool_result() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_1".into()),
                output: json!("file contents"),
                is_error: false,
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["role"], "tool");
        assert_eq!(result["content"], "file contents");
    }

    #[test]
    fn event_run_started_fallback() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "run_started");
    }

    #[test]
    fn source_target_dialects() {
        let mapper = ClaudeToOpenAiMapper;
        assert_eq!(mapper.source_dialect(), Dialect::Claude);
        assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
    }

    // ── Round-trip tests ────────────────────────────────────────────────

    #[test]
    fn roundtrip_simple_message() {
        let openai_req = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hello"}
            ],
            "max_tokens": 512
        });

        let o2c = super::super::OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        // OpenAI → Claude
        let claude_req = o2c
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: openai_req.clone(),
            })
            .unwrap();

        assert_eq!(claude_req["system"], "Be helpful");

        // Claude → OpenAI
        let back = c2o
            .map_request(&DialectRequest {
                dialect: Dialect::Claude,
                body: claude_req,
            })
            .unwrap();

        // Verify structure is preserved
        let msgs = back["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be helpful");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Hello");
        assert_eq!(back["max_tokens"], 512);
    }

    #[test]
    fn roundtrip_tool_definitions() {
        let openai_req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1024,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "search",
                    "description": "Search the web",
                    "parameters": {
                        "type": "object",
                        "properties": {"query": {"type": "string"}}
                    }
                }
            }]
        });

        let o2c = super::super::OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let claude_req = o2c
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: openai_req,
            })
            .unwrap();

        // Claude format has name/description/input_schema
        assert_eq!(claude_req["tools"][0]["name"], "search");
        assert!(claude_req["tools"][0]["input_schema"].is_object());

        let back = c2o
            .map_request(&DialectRequest {
                dialect: Dialect::Claude,
                body: claude_req,
            })
            .unwrap();

        // OpenAI format restored
        assert_eq!(back["tools"][0]["type"], "function");
        assert_eq!(back["tools"][0]["function"]["name"], "search");
        assert!(back["tools"][0]["function"]["parameters"]["properties"]["query"].is_object());
    }

    #[test]
    fn image_content_mapped_to_image_url() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [{
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "What is this?"},
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": "iVBOR..."
                            }
                        }
                    ]
                }]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "user");
        let parts = msg["content"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "What is this?");
        assert_eq!(parts[1]["type"], "image_url");
        assert!(
            parts[1]["image_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
    }
}
