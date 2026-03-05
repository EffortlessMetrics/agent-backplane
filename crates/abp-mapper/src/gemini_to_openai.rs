// SPDX-License-Identifier: MIT OR Apache-2.0

//! Maps Gemini API format to OpenAI chat-completions format.

use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use serde_json::{json, Value};

use crate::{DialectRequest, DialectResponse, Mapper, MappingError};

/// Maps Gemini API requests/responses to OpenAI chat-completions format.
///
/// # Mapping summary
///
/// | Gemini field | OpenAI field | Notes |
/// |---|---|---|
/// | `model` | `model` | Passed through |
/// | `system_instruction` | `messages[0]{role:system}` | Prepended as system message |
/// | `contents[role=model]` | `messages[role=assistant]` | Role renamed |
/// | `contents[role=user]` | `messages[role=user]` | Parts restructured to content |
/// | `tools[].function_declarations` | `tools[].function` | Schema restructured |
/// | `generationConfig.maxOutputTokens` | `max_tokens` | Flattened from config |
/// | `generationConfig.temperature` | `temperature` | Flattened from config |
/// | `generationConfig.topP` | `top_p` | Flattened from config |
/// | `generationConfig.stopSequences` | `stop` | Flattened from config |
///
/// # Examples
///
/// ```
/// use abp_mapper::{Mapper, GeminiToOpenAiMapper, DialectRequest};
/// use abp_dialect::Dialect;
/// use serde_json::json;
///
/// let mapper = GeminiToOpenAiMapper;
/// let req = DialectRequest {
///     dialect: Dialect::Gemini,
///     body: json!({
///         "model": "gemini-pro",
///         "contents": [
///             {"role": "user", "parts": [{"text": "Hello"}]}
///         ]
///     }),
/// };
/// let result = mapper.map_request(&req).unwrap();
/// assert_eq!(result["messages"][0]["role"], "user");
/// assert_eq!(result["messages"][0]["content"], "Hello");
/// ```
pub struct GeminiToOpenAiMapper;

impl Mapper for GeminiToOpenAiMapper {
    fn map_request(&self, from: &DialectRequest) -> Result<Value, MappingError> {
        if from.dialect != Dialect::Gemini {
            return Err(MappingError::UnmappableRequest {
                reason: format!(
                    "GeminiToOpenAiMapper expects Gemini dialect, got {}",
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

        // system_instruction → system message
        if let Some(si) = obj.get("system_instruction") {
            let text = extract_text_from_parts(si);
            if !text.is_empty() {
                openai_messages.push(json!({"role": "system", "content": text}));
            }
        }

        // Convert Gemini contents to OpenAI messages
        if let Some(Value::Array(contents)) = obj.get("contents") {
            for content in contents {
                let role = content.get("role").and_then(Value::as_str).unwrap_or("");
                match role {
                    "user" => {
                        openai_messages.push(map_gemini_user_message(content)?);
                    }
                    "model" => {
                        let mapped = map_gemini_model_message(content)?;
                        openai_messages.extend(mapped);
                    }
                    other => {
                        return Err(MappingError::IncompatibleTypes {
                            source_type: format!("role:{other}"),
                            target_type: "openai_role".into(),
                            reason: format!("unknown Gemini role `{other}`"),
                        });
                    }
                }
            }
        }

        result.insert("messages".into(), Value::Array(openai_messages));

        // generationConfig → flattened fields
        if let Some(config) = obj.get("generationConfig") {
            if let Some(max_tokens) = config.get("maxOutputTokens") {
                result.insert("max_tokens".into(), max_tokens.clone());
            }
            if let Some(temp) = config.get("temperature") {
                result.insert("temperature".into(), temp.clone());
            }
            if let Some(top_p) = config.get("topP") {
                result.insert("top_p".into(), top_p.clone());
            }
            if let Some(stop) = config.get("stopSequences") {
                result.insert("stop".into(), stop.clone());
            }
        }

        // tools: Gemini function_declarations → OpenAI function tools
        if let Some(Value::Array(tool_sets)) = obj.get("tools") {
            let openai_tools = map_tools_gemini_to_openai(tool_sets)?;
            if !openai_tools.is_empty() {
                result.insert("tools".into(), Value::Array(openai_tools));
            }
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
        match &from.kind {
            AgentEventKind::AssistantDelta { text } => Ok(json!({
                "object": "chat.completion.chunk",
                "choices": [{
                    "index": 0,
                    "delta": {"content": text}
                }]
            })),
            AgentEventKind::AssistantMessage { text } => Ok(json!({
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": text},
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
            _ => serde_json::to_value(from).map_err(|e| MappingError::UnmappableRequest {
                reason: format!("failed to serialize event: {e}"),
            }),
        }
    }

    fn source_dialect(&self) -> Dialect {
        Dialect::Gemini
    }

    fn target_dialect(&self) -> Dialect {
        Dialect::OpenAi
    }
}

/// Extracts concatenated text from a Gemini parts-style value.
fn extract_text_from_parts(value: &Value) -> String {
    if let Some(Value::Array(parts)) = value.get("parts") {
        let texts: Vec<String> = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str).map(String::from))
            .collect();
        texts.join("\n\n")
    } else if let Some(s) = value.as_str() {
        s.to_owned()
    } else {
        String::new()
    }
}

/// Maps a Gemini user content entry to an OpenAI message.
fn map_gemini_user_message(content: &Value) -> Result<Value, MappingError> {
    if let Some(Value::Array(parts)) = content.get("parts") {
        // Check for functionResponse parts → emit as tool messages
        let has_fn_response = parts.iter().any(|p| p.get("functionResponse").is_some());

        if has_fn_response && parts.len() == 1 {
            let fr = parts[0].get("functionResponse").unwrap();
            let name = fr.get("name").and_then(Value::as_str).unwrap_or("");
            let response = fr.get("response").cloned().unwrap_or(Value::Null);
            let content_str = match &response {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            return Ok(json!({
                "role": "tool",
                "name": name,
                "content": content_str
            }));
        }

        // Check for mixed content (images)
        let has_inline_data = parts.iter().any(|p| p.get("inlineData").is_some());
        if has_inline_data {
            let openai_parts: Vec<Value> = parts
                .iter()
                .filter_map(|p| {
                    if let Some(text) = p.get("text").and_then(Value::as_str) {
                        Some(json!({"type": "text", "text": text}))
                    } else if let Some(data) = p.get("inlineData") {
                        let mime = data
                            .get("mimeType")
                            .and_then(Value::as_str)
                            .unwrap_or("image/png");
                        let b64 = data.get("data").and_then(Value::as_str).unwrap_or("");
                        let url = format!("data:{mime};base64,{b64}");
                        Some(json!({"type": "image_url", "image_url": {"url": url}}))
                    } else {
                        None
                    }
                })
                .collect();
            return Ok(json!({"role": "user", "content": openai_parts}));
        }

        // Text-only parts
        let text = extract_text_from_parts(content);
        Ok(json!({"role": "user", "content": text}))
    } else {
        Ok(json!({"role": "user", "content": ""}))
    }
}

/// Maps a Gemini model content entry to OpenAI message(s).
///
/// Returns a Vec because a model turn with `functionCall` parts produces
/// an assistant message with `tool_calls`.
fn map_gemini_model_message(content: &Value) -> Result<Vec<Value>, MappingError> {
    let parts = match content.get("parts") {
        Some(Value::Array(p)) => p,
        _ => return Ok(vec![json!({"role": "assistant", "content": ""})]),
    };

    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();

    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            text_parts.push(text.to_owned());
        } else if let Some(fc) = part.get("functionCall") {
            let name = fc
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let args = fc.get("args").cloned().unwrap_or(json!({}));
            let args_str = serde_json::to_string(&args).unwrap_or_else(|_| "{}".into());
            tool_calls.push(json!({
                "id": format!("call_{name}"),
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": args_str
                }
            }));
        }
    }

    let text_content = if text_parts.is_empty() {
        Value::Null
    } else {
        Value::String(text_parts.join("\n"))
    };

    let mut asst = json!({"role": "assistant", "content": text_content});
    if !tool_calls.is_empty() {
        asst["tool_calls"] = Value::Array(tool_calls);
    }

    Ok(vec![asst])
}

/// Maps Gemini tool definitions to OpenAI function-calling format.
fn map_tools_gemini_to_openai(tool_sets: &[Value]) -> Result<Vec<Value>, MappingError> {
    let mut openai_tools = Vec::new();

    for tool_set in tool_sets {
        if let Some(Value::Array(declarations)) = tool_set.get("function_declarations") {
            for decl in declarations {
                let name = decl.get("name").and_then(Value::as_str).unwrap_or("");
                let description = decl
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let parameters = decl
                    .get("parameters")
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
        }
    }

    Ok(openai_tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::AgentEvent;
    use chrono::Utc;

    #[test]
    fn basic_user_message() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [
                    {"role": "user", "parts": [{"text": "Hello"}]}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["model"], "gemini-pro");
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"], "Hello");
    }

    #[test]
    fn system_instruction_becomes_system_message() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "system_instruction": {
                    "parts": [{"text": "You are helpful."}]
                },
                "contents": [
                    {"role": "user", "parts": [{"text": "Hi"}]}
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
    fn model_role_becomes_assistant() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [
                    {"role": "user", "parts": [{"text": "Hi"}]},
                    {"role": "model", "parts": [{"text": "Hello!"}]}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "Hello!");
    }

    #[test]
    fn generation_config_flattened() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "generationConfig": {
                    "maxOutputTokens": 1024,
                    "temperature": 0.7,
                    "topP": 0.9,
                    "stopSequences": ["END"]
                }
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["temperature"], 0.7);
        assert_eq!(result["top_p"], 0.9);
        assert_eq!(result["stop"], json!(["END"]));
    }

    #[test]
    fn function_declarations_to_openai_tools() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "tools": [{
                    "function_declarations": [{
                        "name": "get_weather",
                        "description": "Get weather",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "city": {"type": "string"}
                            }
                        }
                    }]
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
    fn model_with_function_call() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [
                    {"role": "user", "parts": [{"text": "Weather?"}]},
                    {"role": "model", "parts": [{
                        "functionCall": {
                            "name": "get_weather",
                            "args": {"city": "NYC"}
                        }
                    }]}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages[1]["role"], "assistant");
        let tc = &messages[1]["tool_calls"][0];
        assert_eq!(tc["function"]["name"], "get_weather");
    }

    #[test]
    fn function_response_to_tool_message() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [{
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": "get_weather",
                            "response": "72°F"
                        }
                    }]
                }]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "tool");
        assert_eq!(messages[0]["name"], "get_weather");
    }

    #[test]
    fn inline_data_to_image_url() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [{
                    "role": "user",
                    "parts": [
                        {"text": "What is this?"},
                        {"inlineData": {"mimeType": "image/png", "data": "iVBOR..."}}
                    ]
                }]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let msg = &result["messages"][0];
        let parts = msg["content"].as_array().unwrap();
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
        assert!(parts[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn wrong_dialect_rejected() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({"model": "gpt-4"}),
        };
        let err = mapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn non_object_body_rejected() {
        let mapper = GeminiToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!(42),
        };
        let err = mapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn response_tagged_as_openai() {
        let mapper = GeminiToOpenAiMapper;
        let body = json!({"choices": []});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.dialect, Dialect::OpenAi);
    }

    #[test]
    fn event_assistant_delta() {
        let mapper = GeminiToOpenAiMapper;
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
    fn event_tool_call() {
        let mapper = GeminiToOpenAiMapper;
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
        assert_eq!(result["object"], "chat.completion.chunk");
        let tc = &result["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(tc["function"]["name"], "search");
    }

    #[test]
    fn source_target_dialects() {
        let mapper = GeminiToOpenAiMapper;
        assert_eq!(mapper.source_dialect(), Dialect::Gemini);
        assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
    }

    // ── Roundtrip tests ─────────────────────────────────────────────────

    #[test]
    fn roundtrip_simple_message() {
        let o2g = super::super::OpenAiToGeminiMapper;
        let g2o = GeminiToOpenAiMapper;

        let openai_req = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hello"}
            ]
        });

        let gemini_req = o2g
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: openai_req,
            })
            .unwrap();

        assert_eq!(
            gemini_req["system_instruction"]["parts"][0]["text"],
            "Be helpful"
        );

        let back = g2o
            .map_request(&DialectRequest {
                dialect: Dialect::Gemini,
                body: gemini_req,
            })
            .unwrap();

        let msgs = back["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be helpful");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Hello");
    }

    #[test]
    fn roundtrip_tool_definitions() {
        let o2g = super::super::OpenAiToGeminiMapper;
        let g2o = GeminiToOpenAiMapper;

        let openai_req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
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

        let gemini_req = o2g
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: openai_req,
            })
            .unwrap();

        assert!(gemini_req["tools"][0]["function_declarations"][0]["name"] == "search");

        let back = g2o
            .map_request(&DialectRequest {
                dialect: Dialect::Gemini,
                body: gemini_req,
            })
            .unwrap();

        assert_eq!(back["tools"][0]["type"], "function");
        assert_eq!(back["tools"][0]["function"]["name"], "search");
        assert!(back["tools"][0]["function"]["parameters"]["properties"]["query"].is_object());
    }
}
