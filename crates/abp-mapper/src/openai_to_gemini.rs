// SPDX-License-Identifier: MIT OR Apache-2.0

//! Maps OpenAI chat-completions format to Gemini API format.

use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use serde_json::{Value, json};

use crate::{DialectRequest, DialectResponse, Mapper, MappingError};

/// Maps OpenAI chat-completions requests/responses to Gemini API format.
///
/// # Mapping summary
///
/// | OpenAI field | Gemini field | Notes |
/// |---|---|---|
/// | `model` | `model` | Passed through |
/// | `messages[role=system]` | `system_instruction` | Extracted to top-level |
/// | `messages[role=assistant]` | `contents[role=model]` | Role renamed |
/// | `messages[role=user]` | `contents[role=user]` | Content restructured to `parts` |
/// | `messages[role=tool]` | `contents[role=user]` | Wrapped as `functionResponse` part |
/// | `tools` | `tools[0].function_declarations` | Function schema restructured |
/// | `max_tokens` | `generationConfig.maxOutputTokens` | Nested under config |
/// | `temperature` | `generationConfig.temperature` | Nested under config |
/// | `top_p` | `generationConfig.topP` | Nested under config |
/// | `stop` | `generationConfig.stopSequences` | Nested under config |
///
/// # Examples
///
/// ```
/// use abp_mapper::{Mapper, OpenAiToGeminiMapper, DialectRequest};
/// use abp_dialect::Dialect;
/// use serde_json::json;
///
/// let mapper = OpenAiToGeminiMapper;
/// let req = DialectRequest {
///     dialect: Dialect::OpenAi,
///     body: json!({
///         "model": "gpt-4",
///         "messages": [
///             {"role": "user", "content": "Hello"}
///         ]
///     }),
/// };
/// let result = mapper.map_request(&req).unwrap();
/// assert_eq!(result["contents"][0]["role"], "user");
/// assert_eq!(result["contents"][0]["parts"][0]["text"], "Hello");
/// ```
pub struct OpenAiToGeminiMapper;

impl Mapper for OpenAiToGeminiMapper {
    fn map_request(&self, from: &DialectRequest) -> Result<Value, MappingError> {
        if from.dialect != Dialect::OpenAi {
            return Err(MappingError::UnmappableRequest {
                reason: format!(
                    "OpenAiToGeminiMapper expects OpenAI dialect, got {}",
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
            let (system_parts, gemini_contents) = split_messages_for_gemini(messages)?;

            if !system_parts.is_empty() {
                let parts: Vec<Value> = system_parts
                    .into_iter()
                    .map(|text| json!({"text": text}))
                    .collect();
                result.insert("system_instruction".into(), json!({"parts": parts}));
            }

            result.insert("contents".into(), Value::Array(gemini_contents));
        }

        // generationConfig
        let mut gen_config = serde_json::Map::new();
        if let Some(max_tokens) = obj.get("max_tokens") {
            gen_config.insert("maxOutputTokens".into(), max_tokens.clone());
        }
        if let Some(temp) = obj.get("temperature") {
            gen_config.insert("temperature".into(), temp.clone());
        }
        if let Some(top_p) = obj.get("top_p") {
            gen_config.insert("topP".into(), top_p.clone());
        }
        if let Some(stop) = obj.get("stop") {
            match stop {
                Value::String(s) => {
                    gen_config.insert("stopSequences".into(), json!([s]));
                }
                Value::Array(_) => {
                    gen_config.insert("stopSequences".into(), stop.clone());
                }
                _ => {}
            }
        }
        if !gen_config.is_empty() {
            result.insert("generationConfig".into(), Value::Object(gen_config));
        }

        // tools: OpenAI function-calling → Gemini function_declarations
        if let Some(Value::Array(tools)) = obj.get("tools") {
            let declarations = map_tools_openai_to_gemini(tools)?;
            result.insert(
                "tools".into(),
                json!([{"function_declarations": declarations}]),
            );
        }

        Ok(Value::Object(result))
    }

    fn map_response(&self, from: &Value) -> Result<DialectResponse, MappingError> {
        Ok(DialectResponse {
            dialect: Dialect::Gemini,
            body: from.clone(),
        })
    }

    fn map_event(&self, from: &AgentEvent) -> Result<Value, MappingError> {
        match &from.kind {
            AgentEventKind::AssistantDelta { text } => Ok(json!({
                "candidates": [{
                    "content": {
                        "role": "model",
                        "parts": [{"text": text}]
                    }
                }]
            })),
            AgentEventKind::AssistantMessage { text } => Ok(json!({
                "candidates": [{
                    "content": {
                        "role": "model",
                        "parts": [{"text": text}]
                    },
                    "finishReason": "STOP"
                }]
            })),
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => Ok(json!({
                "candidates": [{
                    "content": {
                        "role": "model",
                        "parts": [{
                            "functionCall": {
                                "name": tool_name,
                                "args": input
                            }
                        }]
                    }
                }]
            })),
            AgentEventKind::ToolResult {
                tool_name, output, ..
            } => Ok(json!({
                "candidates": [{
                    "content": {
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": tool_name,
                                "response": output
                            }
                        }]
                    }
                }]
            })),
            _ => serde_json::to_value(from).map_err(|e| MappingError::UnmappableRequest {
                reason: format!("failed to serialize event: {e}"),
            }),
        }
    }

    fn source_dialect(&self) -> Dialect {
        Dialect::OpenAi
    }

    fn target_dialect(&self) -> Dialect {
        Dialect::Gemini
    }
}

/// Splits OpenAI messages into system text and Gemini-formatted contents.
fn split_messages_for_gemini(
    messages: &[Value],
) -> Result<(Vec<String>, Vec<Value>), MappingError> {
    let mut system_parts = Vec::new();
    let mut contents = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");

        match role {
            "system" => {
                if let Some(content) = msg.get("content").and_then(Value::as_str) {
                    system_parts.push(content.to_owned());
                }
            }
            "user" => {
                contents.push(map_user_to_gemini(msg));
            }
            "assistant" => {
                contents.push(map_assistant_to_gemini(msg)?);
            }
            "tool" => {
                contents.push(map_tool_result_to_gemini(msg));
            }
            other => {
                return Err(MappingError::IncompatibleTypes {
                    source_type: format!("role:{other}"),
                    target_type: "gemini_role".into(),
                    reason: format!("unknown OpenAI role `{other}`"),
                });
            }
        }
    }

    Ok((system_parts, contents))
}

/// Maps an OpenAI user message to a Gemini content entry.
fn map_user_to_gemini(msg: &Value) -> Value {
    let content = msg.get("content").cloned().unwrap_or(Value::Null);
    match &content {
        Value::String(text) => json!({
            "role": "user",
            "parts": [{"text": text}]
        }),
        Value::Array(parts) => {
            let gemini_parts: Vec<Value> = parts
                .iter()
                .filter_map(|p| {
                    let ptype = p.get("type").and_then(Value::as_str).unwrap_or("");
                    match ptype {
                        "text" => {
                            let text = p.get("text").and_then(Value::as_str).unwrap_or("");
                            Some(json!({"text": text}))
                        }
                        "image_url" => {
                            // Best-effort: extract base64 data from data URL
                            let url = p
                                .get("image_url")
                                .and_then(|iu| iu.get("url"))
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            if let Some(rest) = url.strip_prefix("data:") {
                                if let Some((mime, data)) = rest.split_once(";base64,") {
                                    return Some(json!({
                                        "inlineData": {
                                            "mimeType": mime,
                                            "data": data
                                        }
                                    }));
                                }
                            }
                            Some(json!({"text": format!("[image: {url}]")}))
                        }
                        _ => None,
                    }
                })
                .collect();
            json!({"role": "user", "parts": gemini_parts})
        }
        _ => json!({
            "role": "user",
            "parts": [{"text": content.to_string()}]
        }),
    }
}

/// Maps an OpenAI assistant message to a Gemini model content entry.
fn map_assistant_to_gemini(msg: &Value) -> Result<Value, MappingError> {
    let mut parts: Vec<Value> = Vec::new();

    // Text content
    if let Some(text) = msg.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            parts.push(json!({"text": text}));
        }
    }

    // tool_calls → functionCall parts
    if let Some(Value::Array(tool_calls)) = msg.get("tool_calls") {
        for tc in tool_calls {
            let function = tc.get("function").unwrap_or(tc);
            let name = function.get("name").and_then(Value::as_str).unwrap_or("");

            let args = if let Some(args_str) = function.get("arguments").and_then(Value::as_str) {
                serde_json::from_str(args_str).unwrap_or(json!({}))
            } else {
                function.get("arguments").cloned().unwrap_or(json!({}))
            };

            parts.push(json!({
                "functionCall": {
                    "name": name,
                    "args": args
                }
            }));
        }
    }

    if parts.is_empty() {
        parts.push(json!({"text": ""}));
    }

    Ok(json!({"role": "model", "parts": parts}))
}

/// Maps an OpenAI tool-result message to a Gemini functionResponse part.
fn map_tool_result_to_gemini(msg: &Value) -> Value {
    let tool_name = msg
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let content = msg.get("content").cloned().unwrap_or(Value::Null);

    json!({
        "role": "user",
        "parts": [{
            "functionResponse": {
                "name": tool_name,
                "response": content
            }
        }]
    })
}

/// Maps OpenAI function-calling tools to Gemini function_declarations format.
fn map_tools_openai_to_gemini(tools: &[Value]) -> Result<Vec<Value>, MappingError> {
    let mut declarations = Vec::new();

    for tool in tools {
        let function =
            tool.get("function")
                .ok_or_else(|| MappingError::IncompatibleTypes {
                    source_type: "openai_tool".into(),
                    target_type: "gemini_function_declaration".into(),
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

        declarations.push(json!({
            "name": name,
            "description": description,
            "parameters": parameters
        }));
    }

    Ok(declarations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::AgentEvent;
    use chrono::Utc;

    #[test]
    fn basic_user_message() {
        let mapper = OpenAiToGeminiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hello"}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["model"], "gpt-4");
        assert_eq!(result["contents"][0]["role"], "user");
        assert_eq!(result["contents"][0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn system_message_to_system_instruction() {
        let mapper = OpenAiToGeminiMapper;
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
        assert_eq!(result["system_instruction"]["parts"][0]["text"], "You are helpful.");
        let contents = result["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
    }

    #[test]
    fn assistant_role_becomes_model() {
        let mapper = OpenAiToGeminiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hi"},
                    {"role": "assistant", "content": "Hello!"}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["contents"][1]["role"], "model");
        assert_eq!(result["contents"][1]["parts"][0]["text"], "Hello!");
    }

    #[test]
    fn generation_config_mapped() {
        let mapper = OpenAiToGeminiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1024,
                "temperature": 0.7,
                "top_p": 0.9,
                "stop": ["END"]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let config = &result["generationConfig"];
        assert_eq!(config["maxOutputTokens"], 1024);
        assert_eq!(config["temperature"], 0.7);
        assert_eq!(config["topP"], 0.9);
        assert_eq!(config["stopSequences"], json!(["END"]));
    }

    #[test]
    fn stop_string_becomes_array() {
        let mapper = OpenAiToGeminiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "stop": "END"
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["generationConfig"]["stopSequences"], json!(["END"]));
    }

    #[test]
    fn tools_to_function_declarations() {
        let mapper = OpenAiToGeminiMapper;
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
                            }
                        }
                    }
                }]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let decls = &result["tools"][0]["function_declarations"];
        assert_eq!(decls[0]["name"], "get_weather");
        assert_eq!(decls[0]["description"], "Get weather for a location");
        assert!(decls[0]["parameters"]["properties"]["location"].is_object());
    }

    #[test]
    fn assistant_with_tool_calls() {
        let mapper = OpenAiToGeminiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Weather?"},
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"city\":\"NYC\"}"
                            }
                        }]
                    }
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let model_msg = &result["contents"][1];
        assert_eq!(model_msg["role"], "model");
        assert_eq!(model_msg["parts"][0]["functionCall"]["name"], "get_weather");
        assert_eq!(model_msg["parts"][0]["functionCall"]["args"]["city"], "NYC");
    }

    #[test]
    fn tool_result_to_function_response() {
        let mapper = OpenAiToGeminiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Weather?"},
                    {
                        "role": "tool",
                        "name": "get_weather",
                        "tool_call_id": "call_1",
                        "content": "72°F, sunny"
                    }
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let tool_msg = &result["contents"][1];
        assert_eq!(tool_msg["role"], "user");
        assert_eq!(
            tool_msg["parts"][0]["functionResponse"]["name"],
            "get_weather"
        );
    }

    #[test]
    fn wrong_dialect_rejected() {
        let mapper = OpenAiToGeminiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({"model": "claude-3"}),
        };
        let err = mapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn non_object_body_rejected() {
        let mapper = OpenAiToGeminiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!("not an object"),
        };
        let err = mapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn response_tagged_as_gemini() {
        let mapper = OpenAiToGeminiMapper;
        let body = json!({"candidates": []});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.dialect, Dialect::Gemini);
    }

    #[test]
    fn event_assistant_delta() {
        let mapper = OpenAiToGeminiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "Hi".into() },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(
            result["candidates"][0]["content"]["parts"][0]["text"],
            "Hi"
        );
        assert_eq!(result["candidates"][0]["content"]["role"], "model");
    }

    #[test]
    fn event_tool_call() {
        let mapper = OpenAiToGeminiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("c1".into()),
                parent_tool_use_id: None,
                input: json!({"q": "rust"}),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        let fc = &result["candidates"][0]["content"]["parts"][0]["functionCall"];
        assert_eq!(fc["name"], "search");
        assert_eq!(fc["args"]["q"], "rust");
    }

    #[test]
    fn source_target_dialects() {
        let mapper = OpenAiToGeminiMapper;
        assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
        assert_eq!(mapper.target_dialect(), Dialect::Gemini);
    }
}
