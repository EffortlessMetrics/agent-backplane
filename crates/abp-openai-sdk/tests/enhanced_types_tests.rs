// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for newly added types: models, stream_options, usage details,
//! parallel_tool_calls, service_tier, and JsonSchema generation.

use abp_openai_sdk::api::{
    AssistantMessage, ChatCompletionRequest, ChatCompletionResponse, Choice,
    CompletionTokensDetails, Delta, FinishReason, FunctionDefinition, Message, PromptTokensDetails,
    StreamChoice, StreamChunk, StreamOptions, Tool, Usage,
};
use abp_openai_sdk::models::{Model, ModelDeleted, ModelList};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// StreamOptions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_options_serde_roundtrip() {
    let opts = StreamOptions {
        include_usage: Some(true),
    };
    let json = serde_json::to_string(&opts).unwrap();
    let parsed: StreamOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, opts);
}

#[test]
fn stream_options_none_usage_omitted() {
    let opts = StreamOptions {
        include_usage: None,
    };
    let json = serde_json::to_string(&opts).unwrap();
    assert!(!json.contains("include_usage"));
}

#[test]
fn request_with_stream_options_roundtrip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![Message::User {
            content: "Hello".into(),
        }],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: Some(true),
        stream_options: Some(StreamOptions {
            include_usage: Some(true),
        }),
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
        parallel_tool_calls: None,
        service_tier: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("stream_options"));
    assert!(json.contains("include_usage"));
    let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

// ═══════════════════════════════════════════════════════════════════════════
// parallel_tool_calls and service_tier
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_parallel_tool_calls_roundtrip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![Message::User {
            content: "test".into(),
        }],
        temperature: None,
        max_tokens: None,
        tools: Some(vec![Tool {
            tool_type: "function".into(),
            function: FunctionDefinition {
                name: "bash".into(),
                description: Some("Run command".into()),
                parameters: Some(json!({"type": "object"})),
                strict: None,
            },
        }]),
        tool_choice: None,
        stream: None,
        stream_options: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
        parallel_tool_calls: Some(true),
        service_tier: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("parallel_tool_calls"));
    let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.parallel_tool_calls, Some(true));
}

#[test]
fn request_service_tier_roundtrip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![Message::User {
            content: "test".into(),
        }],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        stream_options: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
        parallel_tool_calls: None,
        service_tier: Some("auto".into()),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("service_tier"));
    let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.service_tier.as_deref(), Some("auto"));
}

#[test]
fn request_omits_new_none_fields() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        stream_options: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
        parallel_tool_calls: None,
        service_tier: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("stream_options"));
    assert!(!json.contains("parallel_tool_calls"));
    assert!(!json.contains("service_tier"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Usage details (CompletionTokensDetails / PromptTokensDetails)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn completion_tokens_details_serde_roundtrip() {
    let details = CompletionTokensDetails {
        reasoning_tokens: Some(128),
        accepted_prediction_tokens: Some(64),
        rejected_prediction_tokens: Some(16),
    };
    let json = serde_json::to_string(&details).unwrap();
    let parsed: CompletionTokensDetails = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, details);
}

#[test]
fn prompt_tokens_details_serde_roundtrip() {
    let details = PromptTokensDetails {
        cached_tokens: Some(256),
    };
    let json = serde_json::to_string(&details).unwrap();
    let parsed: PromptTokensDetails = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, details);
}

#[test]
fn usage_with_details_serde_roundtrip() {
    let usage = Usage {
        prompt_tokens: 500,
        completion_tokens: 200,
        total_tokens: 700,
        completion_tokens_details: Some(CompletionTokensDetails {
            reasoning_tokens: Some(50),
            accepted_prediction_tokens: None,
            rejected_prediction_tokens: None,
        }),
        prompt_tokens_details: Some(PromptTokensDetails {
            cached_tokens: Some(100),
        }),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
    assert_eq!(
        parsed.completion_tokens_details.unwrap().reasoning_tokens,
        Some(50)
    );
    assert_eq!(
        parsed.prompt_tokens_details.unwrap().cached_tokens,
        Some(100)
    );
}

#[test]
fn usage_without_details_omits_fields() {
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        completion_tokens_details: None,
        prompt_tokens_details: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(!json.contains("completion_tokens_details"));
    assert!(!json.contains("prompt_tokens_details"));
}

#[test]
fn usage_details_deserialize_from_openai_json() {
    let json = r#"{
        "prompt_tokens": 1000,
        "completion_tokens": 500,
        "total_tokens": 1500,
        "completion_tokens_details": {
            "reasoning_tokens": 128,
            "accepted_prediction_tokens": 64,
            "rejected_prediction_tokens": 0
        },
        "prompt_tokens_details": {
            "cached_tokens": 512
        }
    }"#;
    let usage: Usage = serde_json::from_str(json).unwrap();
    assert_eq!(usage.prompt_tokens, 1000);
    assert_eq!(usage.completion_tokens, 500);
    let cd = usage.completion_tokens_details.unwrap();
    assert_eq!(cd.reasoning_tokens, Some(128));
    assert_eq!(cd.accepted_prediction_tokens, Some(64));
    let pd = usage.prompt_tokens_details.unwrap();
    assert_eq!(pd.cached_tokens, Some(512));
}

// ═══════════════════════════════════════════════════════════════════════════
// Model listing types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn model_constructor() {
    let model = Model::new("gpt-4o", "system", 1715367049);
    assert_eq!(model.id, "gpt-4o");
    assert_eq!(model.object, "model");
    assert_eq!(model.owned_by, "system");
    assert_eq!(model.created, 1715367049);
}

#[test]
fn model_serde_roundtrip() {
    let model = Model::new("gpt-4o-mini", "openai", 1721172741);
    let json = serde_json::to_string(&model).unwrap();
    let parsed: Model = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, model);
}

#[test]
fn model_list_constructor_and_serde() {
    let list = ModelList::new(vec![
        Model::new("gpt-4o", "system", 1715367049),
        Model::new("gpt-4o-mini", "system", 1721172741),
        Model::new("gpt-4-turbo", "system", 1700000000),
    ]);
    assert_eq!(list.data.len(), 3);
    assert_eq!(list.object, "list");

    let json = serde_json::to_string(&list).unwrap();
    let parsed: ModelList = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, list);
}

#[test]
fn model_deleted_serde_roundtrip() {
    let deleted = ModelDeleted {
        id: "ft:gpt-4o:org:custom:id".into(),
        object: "model".into(),
        deleted: true,
    };
    let json = serde_json::to_string(&deleted).unwrap();
    let parsed: ModelDeleted = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, deleted);
}

#[test]
fn model_list_empty() {
    let list = ModelList::new(vec![]);
    assert!(list.data.is_empty());
    let json = serde_json::to_string(&list).unwrap();
    assert!(json.contains(r#""data":[]"#));
}

#[test]
fn model_deserializes_from_realistic_openai_json() {
    let json = r#"{
        "id": "gpt-4o-2024-08-06",
        "object": "model",
        "created": 1722814719,
        "owned_by": "system"
    }"#;
    let model: Model = serde_json::from_str(json).unwrap();
    assert_eq!(model.id, "gpt-4o-2024-08-06");
    assert_eq!(model.created, 1722814719);
}

#[test]
fn model_list_deserializes_from_realistic_openai_json() {
    let json = r#"{
        "object": "list",
        "data": [
            {"id": "gpt-4o", "object": "model", "created": 1715367049, "owned_by": "system"},
            {"id": "gpt-4o-mini", "object": "model", "created": 1721172741, "owned_by": "system"},
            {"id": "o1-preview", "object": "model", "created": 1725648897, "owned_by": "system"},
            {"id": "dall-e-3", "object": "model", "created": 1698785189, "owned_by": "system"}
        ]
    }"#;
    let list: ModelList = serde_json::from_str(json).unwrap();
    assert_eq!(list.data.len(), 4);
    assert_eq!(list.data[2].id, "o1-preview");
}

// ═══════════════════════════════════════════════════════════════════════════
// Response with usage details (end-to-end)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_with_detailed_usage_roundtrip() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-detail".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![Choice {
            index: 0,
            message: AssistantMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: FinishReason::Stop,
        }],
        usage: Some(Usage {
            prompt_tokens: 500,
            completion_tokens: 200,
            total_tokens: 700,
            completion_tokens_details: Some(CompletionTokensDetails {
                reasoning_tokens: Some(50),
                accepted_prediction_tokens: None,
                rejected_prediction_tokens: None,
            }),
            prompt_tokens_details: Some(PromptTokensDetails {
                cached_tokens: Some(100),
            }),
        }),
        system_fingerprint: Some("fp_detail".into()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// ═══════════════════════════════════════════════════════════════════════════
// Streaming chunk with usage details
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_chunk_with_detailed_usage() {
    let chunk = StreamChunk {
        id: "chatcmpl-stream-detail".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta::default(),
            finish_reason: Some(FinishReason::Stop),
        }],
        usage: Some(Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            completion_tokens_details: Some(CompletionTokensDetails {
                reasoning_tokens: Some(10),
                accepted_prediction_tokens: None,
                rejected_prediction_tokens: None,
            }),
            prompt_tokens_details: None,
        }),
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

// ═══════════════════════════════════════════════════════════════════════════
// Complete request deserialization from realistic OpenAI JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deserialize_realistic_openai_request() {
    let json = r#"{
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "What is the weather in SF?"}
        ],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get the current weather",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"}
                        },
                        "required": ["location"]
                    }
                }
            }
        ],
        "tool_choice": "auto",
        "temperature": 0.7,
        "max_tokens": 4096,
        "stream": true,
        "stream_options": {"include_usage": true},
        "parallel_tool_calls": true,
        "service_tier": "auto"
    }"#;
    let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.tools.as_ref().unwrap().len(), 1);
    assert_eq!(req.stream, Some(true));
    assert_eq!(
        req.stream_options.as_ref().unwrap().include_usage,
        Some(true)
    );
    assert_eq!(req.parallel_tool_calls, Some(true));
    assert_eq!(req.service_tier.as_deref(), Some("auto"));
}

#[test]
fn deserialize_realistic_openai_response() {
    let json = r#"{
        "id": "chatcmpl-abc123",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "gpt-4o-2024-08-06",
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_abc123",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"location\":\"San Francisco\"}"
                            }
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }
        ],
        "usage": {
            "prompt_tokens": 82,
            "completion_tokens": 17,
            "total_tokens": 99,
            "completion_tokens_details": {
                "reasoning_tokens": 0
            },
            "prompt_tokens_details": {
                "cached_tokens": 0
            }
        },
        "system_fingerprint": "fp_6b68a8219b"
    }"#;
    let resp: ChatCompletionResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.id, "chatcmpl-abc123");
    assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
    let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tc[0].function.name, "get_weather");
    let usage = resp.usage.unwrap();
    assert_eq!(usage.total_tokens, 99);
    assert_eq!(
        usage.completion_tokens_details.unwrap().reasoning_tokens,
        Some(0)
    );
    assert_eq!(resp.system_fingerprint.as_deref(), Some("fp_6b68a8219b"));
}

#[test]
fn deserialize_realistic_streaming_chunk() {
    let json = r#"{
        "id": "chatcmpl-stream-abc",
        "object": "chat.completion.chunk",
        "created": 1700000000,
        "model": "gpt-4o",
        "choices": [
            {
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": "Hello"
                },
                "finish_reason": null
            }
        ]
    }"#;
    let chunk: StreamChunk = serde_json::from_str(json).unwrap();
    assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
    assert!(chunk.choices[0].finish_reason.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// JsonSchema generation smoke tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn json_schema_generation_request() {
    let schema = schemars::schema_for!(ChatCompletionRequest);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("ChatCompletionRequest"));
    assert!(json.contains("model"));
    assert!(json.contains("messages"));
    assert!(json.contains("stream_options"));
    assert!(json.contains("parallel_tool_calls"));
    assert!(json.contains("service_tier"));
}

#[test]
fn json_schema_generation_response() {
    let schema = schemars::schema_for!(ChatCompletionResponse);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("ChatCompletionResponse"));
    assert!(json.contains("choices"));
    assert!(json.contains("usage"));
}

#[test]
fn json_schema_generation_usage() {
    let schema = schemars::schema_for!(Usage);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("prompt_tokens"));
    assert!(json.contains("completion_tokens_details"));
    assert!(json.contains("prompt_tokens_details"));
}

#[test]
fn json_schema_generation_model() {
    let schema = schemars::schema_for!(Model);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("Model"));
    assert!(json.contains("owned_by"));
}

#[test]
fn json_schema_generation_model_list() {
    let schema = schemars::schema_for!(ModelList);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("ModelList"));
}

#[test]
fn json_schema_generation_stream_chunk() {
    let schema = schemars::schema_for!(StreamChunk);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("StreamChunk"));
    assert!(json.contains("delta"));
}

#[test]
fn json_schema_generation_finish_reason() {
    let schema = schemars::schema_for!(FinishReason);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("stop"));
    assert!(json.contains("tool_calls"));
}

#[test]
fn json_schema_generation_stream_options() {
    let schema = schemars::schema_for!(StreamOptions);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("include_usage"));
}
