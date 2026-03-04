// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for `abp_codex_sdk::types`.

use abp_codex_sdk::types::*;
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn minimal_request() -> CodexRequest {
    CodexRequest {
        model: "codex-mini-latest".into(),
        messages: vec![CodexMessage::User {
            content: "Hello".into(),
        }],
        instructions: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    }
}

fn sample_tool() -> CodexTool {
    CodexTool {
        tool_type: "function".into(),
        function: CodexFunctionDef {
            name: "get_weather".into(),
            description: "Get the weather".into(),
            parameters: json!({
                "type": "object",
                "properties": { "location": { "type": "string" } },
                "required": ["location"]
            }),
        },
    }
}

fn sample_response() -> CodexResponse {
    CodexResponse {
        id: "chatcmpl-abc123".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "codex-mini-latest".into(),
        choices: vec![CodexChoice {
            index: 0,
            message: CodexChoiceMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(CodexUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    }
}

// ── CodexRequest tests ──────────────────────────────────────────────────

#[test]
fn request_minimal_serde_roundtrip() {
    let req = minimal_request();
    let json = serde_json::to_string(&req).unwrap();
    let parsed: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn request_omits_none_fields() {
    let req = minimal_request();
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("instructions"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("top_p"));
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("stream"));
    assert!(!json.contains("tools"));
    assert!(!json.contains("tool_choice"));
}

#[test]
fn request_full_serde_roundtrip() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        messages: vec![
            CodexMessage::System {
                content: "You are helpful.".into(),
            },
            CodexMessage::User {
                content: "Fix the bug".into(),
            },
        ],
        instructions: Some("Be concise.".into()),
        temperature: Some(0.7),
        top_p: Some(0.9),
        max_tokens: Some(4096),
        stream: Some(true),
        tools: Some(vec![sample_tool()]),
        tool_choice: Some(CodexToolChoice::Mode(CodexToolChoiceMode::Auto)),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn request_instructions_field_present_in_json() {
    let mut req = minimal_request();
    req.instructions = Some("System prompt from Codex CLI".into());
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("instructions"));
    assert!(json.contains("System prompt from Codex CLI"));
}

// ── CodexMessage tests ──────────────────────────────────────────────────

#[test]
fn message_system_serde_roundtrip() {
    let msg = CodexMessage::System {
        content: "Be helpful.".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"system""#));
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn message_user_serde_roundtrip() {
    let msg = CodexMessage::User {
        content: "Hello!".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"user""#));
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn message_assistant_with_content_serde_roundtrip() {
    let msg = CodexMessage::Assistant {
        content: Some("Response text".into()),
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"assistant""#));
    assert!(!json.contains("tool_calls"));
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn message_assistant_with_tool_calls_serde_roundtrip() {
    let msg = CodexMessage::Assistant {
        content: None,
        tool_calls: Some(vec![CodexToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: CodexFunctionCall {
                name: "shell".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
            },
        }]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains(r#""content""#));
    assert!(json.contains("tool_calls"));
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn message_tool_serde_roundtrip() {
    let msg = CodexMessage::Tool {
        content: "file contents here".into(),
        tool_call_id: "call_1".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"tool""#));
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

// ── CodexResponse tests ─────────────────────────────────────────────────

#[test]
fn response_serde_roundtrip() {
    let resp = sample_response();
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: CodexResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn response_no_usage_omits_field() {
    let mut resp = sample_response();
    resp.usage = None;
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("usage"));
    let parsed: CodexResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// ── CodexStreamChunk tests ──────────────────────────────────────────────

#[test]
fn stream_chunk_serde_roundtrip() {
    let chunk = CodexStreamChunk {
        id: "chatcmpl-stream1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "codex-mini-latest".into(),
        choices: vec![CodexStreamChoice {
            index: 0,
            delta: CodexStreamDelta {
                role: Some("assistant".into()),
                content: Some("Hel".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };
    let json = serde_json::to_string(&chunk).unwrap();
    assert!(json.contains("chat.completion.chunk"));
    let parsed: CodexStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn stream_delta_default_is_empty() {
    let delta = CodexStreamDelta::default();
    assert_eq!(delta.role, None);
    assert_eq!(delta.content, None);
    assert_eq!(delta.tool_calls, None);
    let json = serde_json::to_string(&delta).unwrap();
    assert_eq!(json, "{}");
}

#[test]
fn stream_tool_call_fragment_serde_roundtrip() {
    let fragment = CodexStreamToolCall {
        index: 0,
        id: Some("call_abc".into()),
        call_type: Some("function".into()),
        function: Some(CodexStreamFunctionCall {
            name: Some("shell".into()),
            arguments: Some(r#"{"cmd":"#.into()),
        }),
    };
    let json = serde_json::to_string(&fragment).unwrap();
    assert!(json.contains(r#""type":"function""#));
    let parsed: CodexStreamToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fragment);
}

// ── CodexTool tests ─────────────────────────────────────────────────────

#[test]
fn tool_serde_roundtrip() {
    let tool = sample_tool();
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"function""#));
    let parsed: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn tool_choice_mode_auto() {
    let choice = CodexToolChoice::Mode(CodexToolChoiceMode::Auto);
    let json = serde_json::to_string(&choice).unwrap();
    assert_eq!(json, r#""auto""#);
    let parsed: CodexToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

#[test]
fn tool_choice_function_forced() {
    let choice = CodexToolChoice::Function {
        tool_type: "function".into(),
        function: CodexToolChoiceFunctionRef {
            name: "get_weather".into(),
        },
    };
    let json = serde_json::to_string(&choice).unwrap();
    assert!(json.contains("get_weather"));
    let parsed: CodexToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

// ── CodexFileChange tests ───────────────────────────────────────────────

#[test]
fn file_change_create_serde_roundtrip() {
    let change = CodexFileChange {
        path: "src/main.rs".into(),
        operation: FileOperation::Create,
        content: Some("fn main() {}".into()),
        diff: None,
    };
    let json = serde_json::to_string(&change).unwrap();
    assert!(json.contains(r#""operation":"create""#));
    assert!(!json.contains("diff"));
    let parsed: CodexFileChange = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, change);
}

#[test]
fn file_change_patch_serde_roundtrip() {
    let change = CodexFileChange {
        path: "lib.rs".into(),
        operation: FileOperation::Patch,
        content: None,
        diff: Some("@@ -1,3 +1,3 @@\n-old\n+new".into()),
    };
    let json = serde_json::to_string(&change).unwrap();
    assert!(json.contains(r#""operation":"patch""#));
    assert!(!json.contains(r#""content""#));
    let parsed: CodexFileChange = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, change);
}

#[test]
fn file_change_delete_serde_roundtrip() {
    let change = CodexFileChange {
        path: "old_file.txt".into(),
        operation: FileOperation::Delete,
        content: None,
        diff: None,
    };
    let json = serde_json::to_string(&change).unwrap();
    assert!(json.contains(r#""operation":"delete""#));
    let parsed: CodexFileChange = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, change);
}

#[test]
fn file_operation_all_variants_serialize() {
    let ops = vec![
        (FileOperation::Create, "create"),
        (FileOperation::Update, "update"),
        (FileOperation::Delete, "delete"),
        (FileOperation::Patch, "patch"),
    ];
    for (op, expected) in ops {
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(json, format!(r#""{expected}""#));
    }
}

// ── CodexCommand tests ──────────────────────────────────────────────────

#[test]
fn command_minimal_serde_roundtrip() {
    let cmd = CodexCommand {
        command: "ls -la".into(),
        cwd: None,
        timeout_seconds: None,
        stdout: None,
        stderr: None,
        exit_code: None,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(!json.contains("cwd"));
    assert!(!json.contains("timeout_seconds"));
    assert!(!json.contains("stdout"));
    let parsed: CodexCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cmd);
}

#[test]
fn command_full_serde_roundtrip() {
    let cmd = CodexCommand {
        command: "cargo test".into(),
        cwd: Some("src/".into()),
        timeout_seconds: Some(60),
        stdout: Some("test result: ok".into()),
        stderr: Some(String::new()),
        exit_code: Some(0),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: CodexCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cmd);
}

// ── CodexUsage tests ────────────────────────────────────────────────────

#[test]
fn usage_serde_roundtrip() {
    let usage = CodexUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: CodexUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

// ── Deserialization from raw JSON ───────────────────────────────────────

#[test]
fn response_from_raw_json() {
    let raw = json!({
        "id": "chatcmpl-xyz",
        "object": "chat.completion",
        "created": 1700000000u64,
        "model": "codex-mini-latest",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Done!"
            },
            "finish_reason": "stop"
        }]
    });
    let resp: CodexResponse = serde_json::from_value(raw).unwrap();
    assert_eq!(resp.id, "chatcmpl-xyz");
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Done!"));
    assert!(resp.usage.is_none());
}

#[test]
fn stream_chunk_finish_reason_stop() {
    let chunk = CodexStreamChunk {
        id: "chatcmpl-done".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000001,
        model: "codex-mini-latest".into(),
        choices: vec![CodexStreamChoice {
            index: 0,
            delta: CodexStreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
    };
    let json = serde_json::to_string(&chunk).unwrap();
    assert!(json.contains(r#""finish_reason":"stop""#));
    let parsed: CodexStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.choices[0].finish_reason.as_deref(), Some("stop"));
}
