// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for `DialectDetector` using realistic SDK message examples.

use abp_dialect::{Dialect, DialectDetector};
use serde_json::{Value, json};

fn detector() -> DialectDetector {
    DialectDetector::new()
}

fn detect(v: &Value) -> (Dialect, f64) {
    let r = detector().detect(v).expect("expected detection result");
    (r.dialect, r.confidence)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Real OpenAI examples
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_chat_completion_request() {
    let msg = json!({
        "model": "gpt-4-turbo-2024-04-09",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Explain the difference between TCP and UDP."}
        ],
        "temperature": 0.7,
        "max_tokens": 1024,
        "top_p": 1.0,
        "frequency_penalty": 0.0,
        "presence_penalty": 0.0
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::OpenAi);
    assert!(confidence > 0.5, "confidence {confidence} too low");
}

#[test]
fn openai_chat_completion_response_with_choices() {
    let msg = json!({
        "id": "chatcmpl-abc123",
        "object": "chat.completion",
        "created": 1710000000,
        "model": "gpt-4-turbo-2024-04-09",
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "TCP is a connection-oriented protocol that guarantees delivery, while UDP is connectionless and faster but unreliable."
                },
                "finish_reason": "stop"
            }
        ],
        "usage": {
            "prompt_tokens": 25,
            "completion_tokens": 40,
            "total_tokens": 65
        }
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::OpenAi);
    assert!(confidence >= 0.5, "confidence {confidence} too low");
}

#[test]
fn openai_streaming_chunk_with_delta() {
    let msg = json!({
        "id": "chatcmpl-abc123",
        "object": "chat.completion.chunk",
        "created": 1710000000,
        "model": "gpt-4-turbo-2024-04-09",
        "choices": [
            {
                "index": 0,
                "delta": {
                    "content": "Hello"
                },
                "finish_reason": null
            }
        ]
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::OpenAi);
    assert!(confidence >= 0.5);
}

#[test]
fn openai_function_call_request() {
    let msg = json!({
        "model": "gpt-4-turbo-2024-04-09",
        "messages": [
            {"role": "user", "content": "What is the weather in San Francisco?"}
        ],
        "functions": [
            {
                "name": "get_weather",
                "description": "Get the current weather for a location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string", "description": "City and state"},
                        "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
                    },
                    "required": ["location"]
                }
            }
        ],
        "function_call": "auto"
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::OpenAi);
    assert!(confidence >= 0.5, "confidence {confidence} too low");
}

#[test]
fn openai_tool_call_with_tool_choice() {
    let msg = json!({
        "model": "gpt-4-turbo-2024-04-09",
        "messages": [
            {"role": "user", "content": "Look up the latest stock price for AAPL."}
        ],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "get_stock_price",
                    "description": "Get the latest stock price",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "ticker": {"type": "string"}
                        },
                        "required": ["ticker"]
                    }
                }
            }
        ],
        "tool_choice": {"type": "function", "function": {"name": "get_stock_price"}},
        "temperature": 0.0
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::OpenAi);
    assert!(confidence > 0.5);
}

#[test]
fn openai_response_with_tool_calls() {
    let msg = json!({
        "id": "chatcmpl-xyz789",
        "object": "chat.completion",
        "created": 1710000001,
        "model": "gpt-4-turbo-2024-04-09",
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
                                "name": "get_stock_price",
                                "arguments": "{\"ticker\": \"AAPL\"}"
                            }
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }
        ],
        "usage": {"prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70}
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::OpenAi);
    assert!(confidence >= 0.5);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Real Claude examples
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_messages_request_with_content_blocks() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "Describe the image."},
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": "iVBORw0KGgoAAAANSUhEUg..."
                        }
                    }
                ]
            }
        ]
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Claude);
    assert!(confidence > 0.3);
}

#[test]
fn claude_response_with_stop_reason() {
    let msg = json!({
        "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {
                "type": "text",
                "text": "TCP provides reliable, ordered delivery with connection setup, while UDP is a lightweight protocol that sends datagrams without guarantees."
            }
        ],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 30,
            "output_tokens": 45
        }
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Claude);
    assert!(confidence > 0.5);
}

#[test]
fn claude_thinking_block_content() {
    let msg = json!({
        "id": "msg_thinking_001",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {
                "type": "thinking",
                "thinking": "Let me reason through this step by step..."
            },
            {
                "type": "text",
                "text": "The answer is 42."
            }
        ],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 20, "output_tokens": 60}
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Claude);
    assert!(confidence > 0.5);
}

#[test]
fn claude_tool_use_content_block() {
    let msg = json!({
        "id": "msg_tool_001",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {
                "type": "tool_use",
                "id": "toolu_01A09q90qw90lq917835lq9",
                "name": "get_weather",
                "input": {"location": "San Francisco, CA", "unit": "celsius"}
            }
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 40, "output_tokens": 35}
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Claude);
    assert!(confidence > 0.5);
}

#[test]
fn claude_tool_result_in_messages() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "What is the weather?"}]},
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_01A09q90qw90lq917835lq9",
                        "name": "get_weather",
                        "input": {"location": "San Francisco, CA"}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01A09q90qw90lq917835lq9",
                        "content": "15°C, partly cloudy"
                    }
                ]
            }
        ]
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Claude);
    assert!(confidence > 0.3);
}

#[test]
fn claude_system_prompt_in_separate_field() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 2048,
        "system": "You are a world-class poet. Respond only with short poems.",
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "text": "Write about the ocean."}]
            }
        ]
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Claude);
    assert!(confidence > 0.3);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Real Gemini examples
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_generate_content_request_with_parts() {
    let msg = json!({
        "contents": [
            {
                "role": "user",
                "parts": [
                    {"text": "Explain how photosynthesis works in simple terms."}
                ]
            }
        ],
        "generationConfig": {
            "temperature": 0.7,
            "topP": 0.95,
            "topK": 40,
            "maxOutputTokens": 1024
        }
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Gemini);
    assert!(confidence >= 0.5);
}

#[test]
fn gemini_response_with_candidates() {
    let msg = json!({
        "candidates": [
            {
                "content": {
                    "parts": [
                        {"text": "Photosynthesis converts sunlight, water, and CO2 into glucose and oxygen."}
                    ],
                    "role": "model"
                },
                "finishReason": "STOP",
                "index": 0,
                "safetyRatings": [
                    {"category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE"},
                    {"category": "HARM_CATEGORY_HATE_SPEECH", "probability": "NEGLIGIBLE"}
                ]
            }
        ],
        "usageMetadata": {
            "promptTokenCount": 12,
            "candidatesTokenCount": 30,
            "totalTokenCount": 42
        }
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Gemini);
    assert!(confidence >= 0.4);
}

#[test]
fn gemini_function_call_part() {
    let msg = json!({
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "What is the weather in London?"}]
            }
        ],
        "tools": [
            {
                "functionDeclarations": [
                    {
                        "name": "get_weather",
                        "description": "Returns the weather for a given city",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "city": {"type": "string"}
                            },
                            "required": ["city"]
                        }
                    }
                ]
            }
        ]
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Gemini);
    assert!(confidence >= 0.5);
}

#[test]
fn gemini_function_response_part() {
    let msg = json!({
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "What is the weather?"}]
            },
            {
                "role": "model",
                "parts": [
                    {
                        "functionCall": {
                            "name": "get_weather",
                            "args": {"city": "London"}
                        }
                    }
                ]
            },
            {
                "role": "function",
                "parts": [
                    {
                        "functionResponse": {
                            "name": "get_weather",
                            "response": {"temperature": "12°C", "condition": "cloudy"}
                        }
                    }
                ]
            }
        ]
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Gemini);
    assert!(confidence >= 0.5);
}

#[test]
fn gemini_safety_settings() {
    let msg = json!({
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "Tell me a story."}]
            }
        ],
        "safetySettings": [
            {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_MEDIUM_AND_ABOVE"},
            {"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_MEDIUM_AND_ABOVE"}
        ],
        "generationConfig": {
            "temperature": 0.9
        }
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Gemini);
    assert!(confidence >= 0.5);
}

#[test]
fn gemini_system_instruction() {
    let msg = json!({
        "systemInstruction": {
            "parts": [{"text": "You are a friendly tutor."}]
        },
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "Teach me about gravity."}]
            }
        ],
        "generation_config": {
            "temperature": 0.5,
            "max_output_tokens": 512
        }
    });
    let (dialect, confidence) = detect(&msg);
    assert_eq!(dialect, Dialect::Gemini);
    assert!(confidence >= 0.5);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Ambiguous / edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_json_object() {
    let msg = json!({});
    assert!(detector().detect(&msg).is_none());
    assert!(detector().detect_all(&msg).is_empty());
}

#[test]
fn edge_array_input() {
    let msg = json!([
        {"role": "user", "content": "hello"},
        {"role": "assistant", "content": "hi"}
    ]);
    assert!(detector().detect(&msg).is_none());
}

#[test]
fn edge_nested_messages_multiple_dialect_signals() {
    // Has "messages" with string content (OpenAI signal) and also "contents"
    // with "parts" (Gemini signal). Gemini should win because contents+parts
    // is a stronger signal.
    let msg = json!({
        "model": "some-model",
        "messages": [{"role": "user", "content": "hello"}],
        "contents": [{"parts": [{"text": "hello"}]}],
        "temperature": 0.5
    });
    let results = detector().detect_all(&msg);
    assert!(results.len() >= 2, "should match multiple dialects");
    // Both OpenAI and Gemini should appear
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    assert!(dialects.contains(&Dialect::OpenAi));
    assert!(dialects.contains(&Dialect::Gemini));
}

#[test]
fn edge_minimal_openai_response() {
    // Bare minimum: just "choices" key
    let msg = json!({"choices": []});
    let (dialect, _) = detect(&msg);
    assert_eq!(dialect, Dialect::OpenAi);
}

#[test]
fn edge_minimal_claude_response() {
    // Bare minimum Claude response: type=message
    let msg = json!({"type": "message"});
    let (dialect, _) = detect(&msg);
    assert_eq!(dialect, Dialect::Claude);
}

#[test]
fn edge_non_json_like_object() {
    // An object with no known keys should return None
    let msg = json!({"foo": "bar", "baz": 42});
    assert!(detector().detect(&msg).is_none());
}

#[test]
fn edge_scalar_values_return_none() {
    assert!(detector().detect(&json!(42)).is_none());
    assert!(detector().detect(&json!(true)).is_none());
    assert!(detector().detect(&json!(null)).is_none());
    assert!(detector().detect(&json!("hello world")).is_none());
    assert!(detector().detect(&json!(9.81)).is_none());
}

#[test]
fn edge_deeply_nested_but_no_signals() {
    let msg = json!({
        "data": {
            "inner": {
                "messages": [{"role": "user", "content": "hi"}]
            }
        }
    });
    // The detector only looks at top-level keys, so nested messages don't count.
    assert!(detector().detect(&msg).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Cross-dialect detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_same_prompt_openai_vs_claude() {
    let openai_msg = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "What is 2+2?"}
        ],
        "temperature": 0.0
    });
    let claude_msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 128,
        "system": "You are a helpful assistant.",
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "text": "What is 2+2?"}]
            }
        ]
    });
    let (d1, _) = detect(&openai_msg);
    let (d2, _) = detect(&claude_msg);
    assert_eq!(d1, Dialect::OpenAi);
    assert_eq!(d2, Dialect::Claude);
}

#[test]
fn cross_same_prompt_openai_vs_gemini() {
    let openai_msg = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Summarize quantum computing."}
        ]
    });
    let gemini_msg = json!({
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "Summarize quantum computing."}]
            }
        ],
        "generationConfig": {"temperature": 0.5}
    });
    let (d1, _) = detect(&openai_msg);
    let (d2, _) = detect(&gemini_msg);
    assert_eq!(d1, Dialect::OpenAi);
    assert_eq!(d2, Dialect::Gemini);
}

#[test]
fn cross_same_prompt_claude_vs_gemini() {
    let claude_msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 256,
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "text": "Define entropy."}]
            }
        ]
    });
    let gemini_msg = json!({
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "Define entropy."}]
            }
        ]
    });
    let (d1, _) = detect(&claude_msg);
    let (d2, _) = detect(&gemini_msg);
    assert_eq!(d1, Dialect::Claude);
    assert_eq!(d2, Dialect::Gemini);
}

#[test]
fn cross_confidence_openai_higher_with_more_signals() {
    let minimal = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let full = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{"message": {"role": "assistant", "content": "hello"}}],
        "temperature": 0.7
    });
    let (_, c_min) = detect(&minimal);
    let (_, c_full) = detect(&full);
    assert!(
        c_full > c_min,
        "full message confidence ({c_full}) should exceed minimal ({c_min})"
    );
}

#[test]
fn cross_detect_all_scores_descending() {
    // A message with signals for multiple dialects
    let msg = json!({
        "model": "some-model",
        "messages": [{"role": "user", "content": "hello"}],
        "temperature": 0.5,
        "refs": ["http://example.com"]
    });
    let results = detector().detect_all(&msg);
    assert!(results.len() >= 2, "should match at least 2 dialects");
    for window in results.windows(2) {
        assert!(
            window[0].confidence >= window[1].confidence,
            "results should be sorted descending by confidence"
        );
    }
}

#[test]
fn cross_response_openai_vs_claude() {
    let openai_resp = json!({
        "id": "chatcmpl-resp1",
        "object": "chat.completion",
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "The answer is 4."},
            "finish_reason": "stop"
        }]
    });
    let claude_resp = json!({
        "id": "msg_resp1",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "text", "text": "The answer is 4."}],
        "stop_reason": "end_turn"
    });
    let (d1, _) = detect(&openai_resp);
    let (d2, _) = detect(&claude_resp);
    assert_eq!(d1, Dialect::OpenAi);
    assert_eq!(d2, Dialect::Claude);
}

#[test]
fn cross_evidence_is_always_populated() {
    let examples: Vec<Value> = vec![
        json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
        json!({"type": "message", "content": [{"type": "text", "text": "hi"}]}),
        json!({"contents": [{"parts": [{"text": "hi"}]}]}),
        json!({"items": [{"type": "message"}], "status": "completed"}),
        json!({"references": [{"type": "file"}]}),
    ];
    for example in &examples {
        let result = detector().detect(example).expect("should detect");
        assert!(
            !result.evidence.is_empty(),
            "evidence should be populated for {:?}",
            result.dialect
        );
    }
}
