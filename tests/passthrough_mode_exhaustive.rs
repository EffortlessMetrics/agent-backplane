#![allow(clippy::all)]
#![allow(dead_code, unused_imports, unused_variables)]
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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive passthrough mode verification tests.
//!
//! Validates bitwise equivalence, ABP framing removal, per-dialect field
//! preservation, and edge cases for passthrough mode across all six dialects.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, ReceiptBuilder, RunMetadata, UsageNormalized,
    VerificationReport, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_protocol::Envelope;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn passthrough_receipt(events: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::new_v4(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "passthrough-mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Passthrough,
        usage_raw: serde_json::Value::Null,
        usage,
        trace: events,
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn passthrough_receipt_default(events: Vec<AgentEvent>) -> Receipt {
    passthrough_receipt(events, UsageNormalized::default())
}

fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: Some(id.to_string()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn tool_result_event(
    name: &str,
    id: &str,
    output: serde_json::Value,
    is_error: bool,
) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: name.to_string(),
            tool_use_id: Some(id.to_string()),
            output,
            is_error,
        },
        ext: None,
    }
}

fn error_event(message: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: message.to_string(),
            error_code: None,
        },
        ext: None,
    }
}

fn event_with_raw(kind: AgentEventKind, raw: serde_json::Value) -> AgentEvent {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), raw);
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: Some(ext),
    }
}

fn test_usage(input: u64, output: u64) -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    }
}

/// Serialize a value to bytes and back, verifying identical JSON bytes.
fn assert_bytes_roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) {
    let bytes = serde_json::to_vec(value).unwrap();
    let back: T = serde_json::from_slice(&bytes).unwrap();
    let bytes2 = serde_json::to_vec(&back).unwrap();
    assert_eq!(bytes, bytes2, "byte-level roundtrip mismatch");
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════
    // 1. Bitwise equivalence (15+ tests)
    // ═══════════════════════════════════════════════════════════════════

    mod bitwise_equivalence {
        use super::*;

        #[test]
        fn request_bytes_preserved_through_work_order_roundtrip() {
            let wo = WorkOrderBuilder::new("Write a test")
                .model("gpt-4o")
                .build();
            let bytes = serde_json::to_vec(&wo).unwrap();
            let back: abp_core::WorkOrder = serde_json::from_slice(&bytes).unwrap();
            let bytes2 = serde_json::to_vec(&back).unwrap();
            assert_eq!(bytes, bytes2);
        }

        #[test]
        fn response_bytes_preserved_through_receipt_roundtrip() {
            let receipt = passthrough_receipt_default(vec![assistant_event("Hello!")]);
            assert_bytes_roundtrip(&receipt);
        }

        #[test]
        fn stream_delta_bytes_identical_after_roundtrip() {
            let event = delta_event("token chunk");
            assert_bytes_roundtrip(&event);
        }

        #[test]
        fn tool_call_input_bytes_preserved() {
            let input = json!({"path": "/src/main.rs", "line_start": 1, "line_end": 50});
            let event = tool_call_event("read_file", "call_001", input.clone());
            let bytes = serde_json::to_vec(&event).unwrap();
            let back: AgentEvent = serde_json::from_slice(&bytes).unwrap();
            match &back.kind {
                AgentEventKind::ToolCall {
                    input: back_input, ..
                } => {
                    assert_eq!(
                        serde_json::to_vec(back_input).unwrap(),
                        serde_json::to_vec(&input).unwrap()
                    );
                }
                other => panic!("expected ToolCall, got {other:?}"),
            }
        }

        #[test]
        fn tool_result_output_bytes_preserved() {
            let output = json!({"content": "fn main() {}", "lines": 1});
            let event = tool_result_event("read_file", "call_001", output.clone(), false);
            let bytes = serde_json::to_vec(&event).unwrap();
            let back: AgentEvent = serde_json::from_slice(&bytes).unwrap();
            match &back.kind {
                AgentEventKind::ToolResult {
                    output: back_out, ..
                } => {
                    assert_eq!(
                        serde_json::to_vec(back_out).unwrap(),
                        serde_json::to_vec(&output).unwrap()
                    );
                }
                other => panic!("expected ToolResult, got {other:?}"),
            }
        }

        #[test]
        fn ext_raw_message_bytes_preserved() {
            let raw = json!({
                "id": "chatcmpl-abc123",
                "object": "chat.completion",
                "model": "gpt-4o",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "Hi"}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage { text: "Hi".into() },
                raw.clone(),
            );
            let bytes = serde_json::to_vec(&event).unwrap();
            let back: AgentEvent = serde_json::from_slice(&bytes).unwrap();
            let back_raw = back.ext.as_ref().unwrap().get("raw_message").unwrap();
            assert_eq!(
                serde_json::to_vec(back_raw).unwrap(),
                serde_json::to_vec(&raw).unwrap()
            );
        }

        #[test]
        fn multiple_ext_keys_bytes_preserved() {
            let mut ext = BTreeMap::new();
            ext.insert("vendor_id".to_string(), json!("resp_12345"));
            ext.insert("latency_ms".to_string(), json!(142));
            ext.insert("raw_headers".to_string(), json!({"x-request-id": "abc"}));
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "ok".into() },
                ext: Some(ext.clone()),
            };
            let bytes = serde_json::to_vec(&event).unwrap();
            let back: AgentEvent = serde_json::from_slice(&bytes).unwrap();
            let back_ext = back.ext.as_ref().unwrap();
            for (key, val) in &ext {
                assert_eq!(
                    serde_json::to_vec(back_ext.get(key).unwrap()).unwrap(),
                    serde_json::to_vec(val).unwrap(),
                    "ext key '{key}' byte mismatch"
                );
            }
        }

        #[test]
        fn usage_raw_bytes_preserved() {
            let usage_raw = json!({
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
                "prompt_tokens_details": {"cached_tokens": 10}
            });
            let receipt = Receipt {
                usage_raw: usage_raw.clone(),
                ..passthrough_receipt_default(vec![assistant_event("ok")])
            };
            let bytes = serde_json::to_vec(&receipt).unwrap();
            let back: Receipt = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                serde_json::to_vec(&back.usage_raw).unwrap(),
                serde_json::to_vec(&usage_raw).unwrap()
            );
        }

        #[test]
        fn verification_report_bytes_preserved() {
            let receipt = Receipt {
                verification: VerificationReport {
                    git_diff: Some("diff --git a/f.rs b/f.rs\n+new line".into()),
                    git_status: Some("M src/f.rs\n".into()),
                    harness_ok: true,
                },
                ..passthrough_receipt_default(vec![])
            };
            assert_bytes_roundtrip(&receipt);
        }

        #[test]
        fn artifact_refs_bytes_preserved() {
            let receipt = Receipt {
                artifacts: vec![
                    ArtifactRef {
                        kind: "patch".into(),
                        path: "output.patch".into(),
                    },
                    ArtifactRef {
                        kind: "log".into(),
                        path: "run.log".into(),
                    },
                ],
                ..passthrough_receipt_default(vec![])
            };
            assert_bytes_roundtrip(&receipt);
        }

        #[test]
        fn streaming_event_sequence_bytes_identical() {
            let events = vec![
                delta_event("The "),
                delta_event("quick "),
                delta_event("brown "),
                delta_event("fox"),
            ];
            for event in &events {
                assert_bytes_roundtrip(event);
            }
        }

        #[test]
        fn receipt_trace_order_byte_identical() {
            let events = vec![
                assistant_event("first"),
                tool_call_event("bash", "c1", json!({"cmd": "ls"})),
                tool_result_event("bash", "c1", json!("files"), false),
                assistant_event("second"),
            ];
            let receipt = passthrough_receipt_default(events);
            let bytes = serde_json::to_vec(&receipt).unwrap();
            let back: Receipt = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(receipt.trace.len(), back.trace.len());
            for (orig, roundtripped) in receipt.trace.iter().zip(back.trace.iter()) {
                assert_eq!(
                    serde_json::to_vec(&orig.kind).unwrap(),
                    serde_json::to_vec(&roundtripped.kind).unwrap()
                );
            }
        }

        #[test]
        fn command_executed_event_bytes_preserved() {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::CommandExecuted {
                    command: "cargo test --release".into(),
                    exit_code: Some(0),
                    output_preview: Some("test result: ok. 42 passed".into()),
                },
                ext: None,
            };
            assert_bytes_roundtrip(&event);
        }

        #[test]
        fn file_changed_event_bytes_preserved() {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: "src/lib.rs".into(),
                    summary: "Added error handling to parse function".into(),
                },
                ext: None,
            };
            assert_bytes_roundtrip(&event);
        }

        #[test]
        fn warning_event_bytes_preserved() {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Warning {
                    message: "Deprecated API call detected".into(),
                },
                ext: None,
            };
            assert_bytes_roundtrip(&event);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 2. ABP framing removal (10+ tests)
    // ═══════════════════════════════════════════════════════════════════

    mod abp_framing_removal {
        use super::*;

        #[test]
        fn event_envelope_wrapping_does_not_alter_event_payload() {
            let event = assistant_event("Hello world");
            let event_json = serde_json::to_value(&event).unwrap();

            let envelope = Envelope::Event {
                ref_id: "run_123".into(),
                event: event.clone(),
            };
            let envelope_json = serde_json::to_value(&envelope).unwrap();

            // The inner event field in the envelope must match the standalone event.
            let inner = &envelope_json["event"];
            assert_eq!(inner["type"], event_json["type"]);
            assert_eq!(inner["text"], event_json["text"]);
        }

        #[test]
        fn final_envelope_receipt_matches_standalone_receipt() {
            let receipt = passthrough_receipt_default(vec![assistant_event("done")]);
            let receipt_json = serde_json::to_value(&receipt).unwrap();

            let envelope = Envelope::Final {
                ref_id: "run_456".into(),
                receipt: receipt.clone(),
            };
            let envelope_json = serde_json::to_value(&envelope).unwrap();

            let inner = &envelope_json["receipt"];
            assert_eq!(inner["mode"], receipt_json["mode"]);
            assert_eq!(inner["outcome"], receipt_json["outcome"]);
            assert_eq!(
                inner["meta"]["contract_version"],
                receipt_json["meta"]["contract_version"]
            );
        }

        #[test]
        fn envelope_t_field_does_not_leak_into_receipt() {
            let receipt = passthrough_receipt_default(vec![]);
            let receipt_json = serde_json::to_value(&receipt).unwrap();
            // The receipt itself should never contain a "t" discriminator field.
            assert!(receipt_json.get("t").is_none());
        }

        #[test]
        fn envelope_ref_id_does_not_leak_into_event() {
            let event = delta_event("chunk");
            let event_json = serde_json::to_value(&event).unwrap();
            // Events should not contain ref_id — that's envelope-level framing.
            assert!(event_json.get("ref_id").is_none());
        }

        #[test]
        fn run_envelope_work_order_matches_standalone() {
            let wo = WorkOrderBuilder::new("Test task").model("gpt-4o").build();
            let wo_json = serde_json::to_value(&wo).unwrap();

            let envelope = Envelope::Run {
                id: "run_789".into(),
                work_order: wo.clone(),
            };
            let envelope_json = serde_json::to_value(&envelope).unwrap();

            let inner = &envelope_json["work_order"];
            assert_eq!(inner["task"], wo_json["task"]);
            assert_eq!(inner["config"]["model"], wo_json["config"]["model"]);
        }

        #[test]
        fn receipt_metadata_absent_from_event_stream() {
            let events = vec![delta_event("Hello"), delta_event(" world")];
            for event in &events {
                let json = serde_json::to_value(event).unwrap();
                // Receipt-only fields should never appear in events.
                assert!(json.get("outcome").is_none());
                assert!(json.get("receipt_sha256").is_none());
                assert!(json.get("usage_raw").is_none());
                assert!(json.get("backend").is_none());
                assert!(json.get("artifacts").is_none());
            }
        }

        #[test]
        fn work_order_metadata_absent_from_response_events() {
            let event = assistant_event("Response text");
            let json = serde_json::to_value(&event).unwrap();
            // Work order fields should never leak into response events.
            assert!(json.get("task").is_none());
            assert!(json.get("lane").is_none());
            assert!(json.get("workspace").is_none());
            assert!(json.get("policy").is_none());
            assert!(json.get("requirements").is_none());
        }

        #[test]
        fn hello_envelope_fields_do_not_leak_into_receipt() {
            let hello = Envelope::Hello {
                contract_version: CONTRACT_VERSION.into(),
                backend: BackendIdentity {
                    id: "sidecar:node".into(),
                    backend_version: Some("1.0".into()),
                    adapter_version: None,
                },
                capabilities: CapabilityManifest::new(),
                mode: ExecutionMode::Passthrough,
            };
            let hello_json = serde_json::to_value(&hello).unwrap();
            // Hello envelope has a "t" tag that must not propagate.
            assert_eq!(hello_json["t"], json!("hello"));

            let receipt = passthrough_receipt_default(vec![]);
            let receipt_json = serde_json::to_value(&receipt).unwrap();
            assert!(receipt_json.get("t").is_none());
        }

        #[test]
        fn fatal_envelope_error_separate_from_event_errors() {
            let fatal = Envelope::Fatal {
                ref_id: Some("run_1".into()),
                error: "sidecar crashed".into(),
                error_code: None,
            };
            let fatal_json = serde_json::to_value(&fatal).unwrap();
            assert_eq!(fatal_json["t"], json!("fatal"));

            // An error AgentEvent should not look like a fatal envelope.
            let error_evt = error_event("sidecar crashed");
            let error_json = serde_json::to_value(&error_evt).unwrap();
            assert!(error_json.get("t").is_none());
            assert!(error_json.get("ref_id").is_none());
        }

        #[test]
        fn stripping_envelope_yields_exact_inner_payload() {
            let event = tool_call_event("bash", "c1", json!({"cmd": "ls"}));
            let envelope = Envelope::Event {
                ref_id: "run_abc".into(),
                event: event.clone(),
            };
            let envelope_json = serde_json::to_value(&envelope).unwrap();

            // Extract the inner event from the envelope.
            let extracted: AgentEvent =
                serde_json::from_value(envelope_json["event"].clone()).unwrap();
            let orig_bytes = serde_json::to_vec(&event.kind).unwrap();
            let extracted_bytes = serde_json::to_vec(&extracted.kind).unwrap();
            assert_eq!(orig_bytes, extracted_bytes);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 3. All 6 dialects in passthrough (12+ tests)
    // ═══════════════════════════════════════════════════════════════════

    mod openai_dialect_passthrough {
        use super::*;

        #[test]
        fn openai_raw_response_preserved_in_ext() {
            let raw = json!({
                "id": "chatcmpl-9abc",
                "object": "chat.completion",
                "created": 1700000000,
                "model": "gpt-4o-2024-11-20",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello!",
                        "refusal": null
                    },
                    "logprobs": null,
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15,
                    "prompt_tokens_details": {"cached_tokens": 0},
                    "completion_tokens_details": {"reasoning_tokens": 0}
                },
                "system_fingerprint": "fp_abc123"
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage {
                    text: "Hello!".into(),
                },
                raw.clone(),
            );
            let receipt = passthrough_receipt_default(vec![event]);
            let stored = receipt.trace[0]
                .ext
                .as_ref()
                .unwrap()
                .get("raw_message")
                .unwrap();
            assert_eq!(stored, &raw);
            assert_eq!(stored["system_fingerprint"], "fp_abc123");
            assert_eq!(stored["choices"][0]["logprobs"], json!(null));
            assert_eq!(stored["choices"][0]["message"]["refusal"], json!(null));
        }

        #[test]
        fn openai_streaming_chunk_fields_preserved() {
            let raw_chunk = json!({
                "id": "chatcmpl-stream1",
                "object": "chat.completion.chunk",
                "created": 1700000001,
                "model": "gpt-4o",
                "choices": [{"index": 0, "delta": {"content": "tok"}, "finish_reason": null}]
            });
            let event = event_with_raw(
                AgentEventKind::AssistantDelta { text: "tok".into() },
                raw_chunk.clone(),
            );
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            let back_raw = back.ext.as_ref().unwrap().get("raw_message").unwrap();
            assert_eq!(back_raw["object"], "chat.completion.chunk");
            assert_eq!(back_raw["choices"][0]["delta"]["content"], "tok");
            assert_eq!(back_raw["choices"][0]["finish_reason"], json!(null));
        }
    }

    mod claude_dialect_passthrough {
        use super::*;

        #[test]
        fn claude_raw_response_preserved_in_ext() {
            let raw = json!({
                "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "thinking",
                    "thinking": "Let me consider...",
                    "signature": "sig_v1_abc"
                }, {
                    "type": "text",
                    "text": "The answer is 42."
                }],
                "model": "claude-sonnet-4-20250514",
                "stop_reason": "end_turn",
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 50,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 0
                }
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage {
                    text: "The answer is 42.".into(),
                },
                raw.clone(),
            );
            let receipt = passthrough_receipt_default(vec![event]);
            let stored = receipt.trace[0]
                .ext
                .as_ref()
                .unwrap()
                .get("raw_message")
                .unwrap();
            assert_eq!(stored, &raw);
            assert_eq!(stored["content"][0]["type"], "thinking");
            assert_eq!(stored["content"][0]["signature"], "sig_v1_abc");
            assert_eq!(stored["stop_sequence"], json!(null));
        }

        #[test]
        fn claude_tool_use_block_preserved() {
            let raw = json!({
                "type": "tool_use",
                "id": "toolu_01A09q90qw90lq917835lqs8",
                "name": "bash",
                "input": {"command": "ls -la /tmp"}
            });
            let event = event_with_raw(
                AgentEventKind::ToolCall {
                    tool_name: "bash".into(),
                    tool_use_id: Some("toolu_01A09q90qw90lq917835lqs8".into()),
                    parent_tool_use_id: None,
                    input: json!({"command": "ls -la /tmp"}),
                },
                raw.clone(),
            );
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            assert_eq!(back.ext.as_ref().unwrap().get("raw_message").unwrap(), &raw);
        }
    }

    mod gemini_dialect_passthrough {
        use super::*;

        #[test]
        fn gemini_raw_response_preserved_in_ext() {
            let raw = json!({
                "candidates": [{
                    "content": {
                        "parts": [{"text": "Hello from Gemini!"}],
                        "role": "model"
                    },
                    "finishReason": "STOP",
                    "index": 0,
                    "safetyRatings": [
                        {"category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE"}
                    ]
                }],
                "usageMetadata": {
                    "promptTokenCount": 10,
                    "candidatesTokenCount": 8,
                    "totalTokenCount": 18
                },
                "modelVersion": "gemini-2.5-flash"
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage {
                    text: "Hello from Gemini!".into(),
                },
                raw.clone(),
            );
            let receipt = passthrough_receipt_default(vec![event]);
            let stored = receipt.trace[0]
                .ext
                .as_ref()
                .unwrap()
                .get("raw_message")
                .unwrap();
            assert_eq!(stored, &raw);
            assert_eq!(stored["candidates"][0]["finishReason"], "STOP");
            assert_eq!(stored["usageMetadata"]["totalTokenCount"], 18);
            assert!(stored["candidates"][0]["safetyRatings"].is_array());
        }

        #[test]
        fn gemini_function_call_response_preserved() {
            let raw = json!({
                "candidates": [{
                    "content": {
                        "parts": [{
                            "functionCall": {
                                "name": "get_weather",
                                "args": {"location": "San Francisco", "unit": "celsius"}
                            }
                        }],
                        "role": "model"
                    },
                    "finishReason": "STOP"
                }]
            });
            let event = event_with_raw(
                AgentEventKind::ToolCall {
                    tool_name: "get_weather".into(),
                    tool_use_id: None,
                    parent_tool_use_id: None,
                    input: json!({"location": "San Francisco", "unit": "celsius"}),
                },
                raw.clone(),
            );
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            let back_raw = back.ext.as_ref().unwrap().get("raw_message").unwrap();
            assert_eq!(
                back_raw["candidates"][0]["content"]["parts"][0]["functionCall"]["name"],
                "get_weather"
            );
        }
    }

    mod codex_dialect_passthrough {
        use super::*;

        #[test]
        fn codex_raw_response_preserved_in_ext() {
            let raw = json!({
                "id": "resp_abc123",
                "object": "response",
                "created_at": 1700000000,
                "status": "completed",
                "model": "codex-mini-latest",
                "output": [{
                    "type": "message",
                    "id": "msg_001",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "fn main() {}"}]
                }],
                "usage": {
                    "input_tokens": 50,
                    "output_tokens": 25,
                    "total_tokens": 75
                }
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage {
                    text: "fn main() {}".into(),
                },
                raw.clone(),
            );
            let receipt = passthrough_receipt_default(vec![event]);
            let stored = receipt.trace[0]
                .ext
                .as_ref()
                .unwrap()
                .get("raw_message")
                .unwrap();
            assert_eq!(stored, &raw);
            assert_eq!(stored["status"], "completed");
            assert_eq!(stored["output"][0]["type"], "message");
        }

        #[test]
        fn codex_function_call_output_preserved() {
            let raw = json!({
                "type": "function_call",
                "id": "fc_001",
                "call_id": "call_abc",
                "name": "shell",
                "arguments": "{\"command\":\"cargo build\"}"
            });
            let event = event_with_raw(
                AgentEventKind::ToolCall {
                    tool_name: "shell".into(),
                    tool_use_id: Some("call_abc".into()),
                    parent_tool_use_id: None,
                    input: json!({"command": "cargo build"}),
                },
                raw.clone(),
            );
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            assert_eq!(back.ext.as_ref().unwrap().get("raw_message").unwrap(), &raw);
        }
    }

    mod kimi_dialect_passthrough {
        use super::*;

        #[test]
        fn kimi_raw_response_preserved_in_ext() {
            let raw = json!({
                "id": "cmpl-kimi-abc",
                "object": "chat.completion",
                "created": 1700000000,
                "model": "moonshot-v1-128k",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Rust is a systems programming language."
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 15,
                    "completion_tokens": 10,
                    "total_tokens": 25
                }
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage {
                    text: "Rust is a systems programming language.".into(),
                },
                raw.clone(),
            );
            let receipt = passthrough_receipt_default(vec![event]);
            let stored = receipt.trace[0]
                .ext
                .as_ref()
                .unwrap()
                .get("raw_message")
                .unwrap();
            assert_eq!(stored, &raw);
            assert_eq!(stored["model"], "moonshot-v1-128k");
        }

        #[test]
        fn kimi_web_search_tool_call_preserved() {
            let raw = json!({
                "type": "tool_calls",
                "tool_calls": [{
                    "id": "call_ws1",
                    "type": "function",
                    "function": {
                        "name": "web_search",
                        "arguments": "{\"query\": \"Rust async runtime\"}"
                    }
                }]
            });
            let event = event_with_raw(
                AgentEventKind::ToolCall {
                    tool_name: "web_search".into(),
                    tool_use_id: Some("call_ws1".into()),
                    parent_tool_use_id: None,
                    input: json!({"query": "Rust async runtime"}),
                },
                raw.clone(),
            );
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            assert_eq!(back.ext.as_ref().unwrap().get("raw_message").unwrap(), &raw);
        }
    }

    mod copilot_dialect_passthrough {
        use super::*;

        #[test]
        fn copilot_raw_response_preserved_in_ext() {
            let raw = json!({
                "type": "copilot.completion",
                "message": "I can help with that refactoring!",
                "references": [
                    {"type": "file", "path": "src/main.rs", "start_line": 1, "end_line": 10}
                ],
                "function_call": null,
                "copilot_errors": []
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage {
                    text: "I can help with that refactoring!".into(),
                },
                raw.clone(),
            );
            let receipt = passthrough_receipt_default(vec![event]);
            let stored = receipt.trace[0]
                .ext
                .as_ref()
                .unwrap()
                .get("raw_message")
                .unwrap();
            assert_eq!(stored, &raw);
            assert_eq!(stored["function_call"], json!(null));
            assert!(stored["references"].is_array());
            assert!(stored["copilot_errors"].as_array().unwrap().is_empty());
        }

        #[test]
        fn copilot_function_call_preserved() {
            let raw = json!({
                "type": "copilot.function_call",
                "function_call": {
                    "name": "read_file",
                    "id": "call_rf1",
                    "arguments": "{\"path\":\"src/lib.rs\"}"
                }
            });
            let event = event_with_raw(
                AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("call_rf1".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "src/lib.rs"}),
                },
                raw.clone(),
            );
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            assert_eq!(back.ext.as_ref().unwrap().get("raw_message").unwrap(), &raw);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 4. Edge cases (10+ tests)
    // ═══════════════════════════════════════════════════════════════════

    mod edge_cases {
        use super::*;

        #[test]
        fn empty_response_trace_roundtrip() {
            let receipt = passthrough_receipt_default(vec![]);
            let json_str = serde_json::to_string(&receipt).unwrap();
            let back: Receipt = serde_json::from_str(&json_str).unwrap();
            assert!(back.trace.is_empty());
            assert_eq!(back.mode, ExecutionMode::Passthrough);
        }

        #[test]
        fn empty_string_content_preserved() {
            let event = assistant_event("");
            let receipt = passthrough_receipt_default(vec![event]);
            match &receipt.trace[0].kind {
                AgentEventKind::AssistantMessage { text } => assert_eq!(text, ""),
                other => panic!("expected AssistantMessage, got {other:?}"),
            }
        }

        #[test]
        fn binary_content_as_base64_preserved() {
            // Simulate binary data transported as base64 in tool output.
            let binary_bytes: Vec<u8> = (0..=255).collect();
            let b64 = binary_bytes
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let event = tool_result_event(
                "read_binary",
                "call_bin",
                json!({"data": b64, "encoding": "hex", "size": 256}),
                false,
            );
            let receipt = passthrough_receipt_default(vec![event]);
            match &receipt.trace[0].kind {
                AgentEventKind::ToolResult { output, .. } => {
                    assert_eq!(output["data"].as_str().unwrap(), b64);
                    assert_eq!(output["size"], 256);
                }
                other => panic!("expected ToolResult, got {other:?}"),
            }
        }

        #[test]
        fn very_large_payload_preserved() {
            let large_text = "A".repeat(5_000_000);
            let event = assistant_event(&large_text);
            let receipt = passthrough_receipt_default(vec![event]);
            match &receipt.trace[0].kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert_eq!(text.len(), 5_000_000);
                }
                other => panic!("expected AssistantMessage, got {other:?}"),
            }
        }

        #[test]
        fn very_large_ext_payload_preserved() {
            let large_raw = json!({
                "data": "B".repeat(1_000_000),
                "nested": { "arr": (0..1000).collect::<Vec<i32>>() }
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage { text: "ok".into() },
                large_raw.clone(),
            );
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            assert_eq!(
                back.ext.as_ref().unwrap().get("raw_message").unwrap(),
                &large_raw
            );
        }

        #[test]
        fn unicode_in_all_event_fields() {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "読み取り".to_string(),
                    tool_use_id: Some("呼び出し_①".to_string()),
                    parent_tool_use_id: Some("親_ツール".to_string()),
                    input: json!({"パス": "ソース/メイン.rs", "エンコーディング": "utf-8"}),
                },
                ext: None,
            };
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            match &back.kind {
                AgentEventKind::ToolCall {
                    tool_name,
                    tool_use_id,
                    parent_tool_use_id,
                    input,
                } => {
                    assert_eq!(tool_name, "読み取り");
                    assert_eq!(tool_use_id.as_deref(), Some("呼び出し_①"));
                    assert_eq!(parent_tool_use_id.as_deref(), Some("親_ツール"));
                    assert_eq!(input["パス"], "ソース/メイン.rs");
                }
                other => panic!("expected ToolCall, got {other:?}"),
            }
        }

        #[test]
        fn emoji_and_multibyte_preserved() {
            let texts = vec![
                "🦀 Rust is awesome! 🚀",
                "café résumé naïve",
                "中文测试 日本語テスト 한국어테스트",
                "𝕳𝖊𝖑𝖑𝖔 𝖂𝖔𝖗𝖑𝖉",
                "\u{200B}zero-width\u{200B}spaces\u{200B}",
                "RTL: مرحبا بالعالم",
            ];
            for text in &texts {
                let event = assistant_event(text);
                let json_str = serde_json::to_string(&event).unwrap();
                let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
                match &back.kind {
                    AgentEventKind::AssistantMessage { text: t } => assert_eq!(t, text),
                    other => panic!("expected AssistantMessage, got {other:?}"),
                }
            }
        }

        #[test]
        fn null_fields_preserved_in_ext() {
            let mut ext = BTreeMap::new();
            ext.insert("null_val".to_string(), serde_json::Value::Null);
            ext.insert(
                "nested_nulls".to_string(),
                json!({"a": null, "b": {"c": null}}),
            );
            ext.insert("array_with_nulls".to_string(), json!([null, 1, null, "x"]));
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "ok".into() },
                ext: Some(ext),
            };
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            let back_ext = back.ext.as_ref().unwrap();
            assert_eq!(back_ext["null_val"], json!(null));
            assert_eq!(back_ext["nested_nulls"]["a"], json!(null));
            assert_eq!(back_ext["nested_nulls"]["b"]["c"], json!(null));
            assert_eq!(back_ext["array_with_nulls"][0], json!(null));
            assert_eq!(back_ext["array_with_nulls"][2], json!(null));
        }

        #[test]
        fn null_optional_fields_on_events_preserved() {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "bash".into(),
                    tool_use_id: None,
                    parent_tool_use_id: None,
                    input: json!(null),
                },
                ext: None,
            };
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            match &back.kind {
                AgentEventKind::ToolCall {
                    tool_use_id,
                    parent_tool_use_id,
                    input,
                    ..
                } => {
                    assert!(tool_use_id.is_none());
                    assert!(parent_tool_use_id.is_none());
                    assert_eq!(input, &json!(null));
                }
                other => panic!("expected ToolCall, got {other:?}"),
            }
        }

        #[test]
        fn special_json_characters_in_strings_preserved() {
            let tricky = "Line1\nLine2\tTabbed\r\nWindows\\path\"quoted\"";
            let event = assistant_event(tricky);
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            match &back.kind {
                AgentEventKind::AssistantMessage { text } => assert_eq!(text, tricky),
                other => panic!("expected AssistantMessage, got {other:?}"),
            }
        }

        #[test]
        fn numeric_precision_in_ext_preserved() {
            let mut ext = BTreeMap::new();
            ext.insert("integer".to_string(), json!(9007199254740993_i64));
            ext.insert("float".to_string(), json!(3.141592653589793));
            ext.insert("negative".to_string(), json!(-1));
            ext.insert("zero".to_string(), json!(0));
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "ok".into() },
                ext: Some(ext),
            };
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            let back_ext = back.ext.as_ref().unwrap();
            assert_eq!(back_ext["integer"], json!(9007199254740993_i64));
            assert_eq!(back_ext["float"], json!(3.141592653589793));
            assert_eq!(back_ext["negative"], json!(-1));
            assert_eq!(back_ext["zero"], json!(0));
        }

        #[test]
        fn many_small_deltas_preserved_exactly() {
            let events: Vec<AgentEvent> = (0..500).map(|i| delta_event(&i.to_string())).collect();
            let receipt = passthrough_receipt_default(events);
            assert_eq!(receipt.trace.len(), 500);
            for (i, evt) in receipt.trace.iter().enumerate() {
                match &evt.kind {
                    AgentEventKind::AssistantDelta { text } => {
                        assert_eq!(text, &i.to_string());
                    }
                    other => panic!("index {i}: expected AssistantDelta, got {other:?}"),
                }
            }
        }

        #[test]
        fn receipt_with_all_none_usage_fields_preserved() {
            let usage = UsageNormalized {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: None,
            };
            let receipt = passthrough_receipt(vec![], usage);
            let json_str = serde_json::to_string(&receipt).unwrap();
            let back: Receipt = serde_json::from_str(&json_str).unwrap();
            assert!(back.usage.input_tokens.is_none());
            assert!(back.usage.output_tokens.is_none());
            assert!(back.usage.cache_read_tokens.is_none());
            assert!(back.usage.cache_write_tokens.is_none());
            assert!(back.usage.request_units.is_none());
            assert!(back.usage.estimated_cost_usd.is_none());
        }

        #[test]
        fn mixed_event_types_with_ext_all_preserved() {
            let events = vec![
                event_with_raw(
                    AgentEventKind::RunStarted {
                        message: "starting".into(),
                    },
                    json!({"vendor_trace_id": "trace_001"}),
                ),
                event_with_raw(
                    AgentEventKind::AssistantDelta { text: "Hi".into() },
                    json!({"chunk_index": 0}),
                ),
                event_with_raw(
                    AgentEventKind::ToolCall {
                        tool_name: "bash".into(),
                        tool_use_id: Some("c1".into()),
                        parent_tool_use_id: None,
                        input: json!({"cmd": "echo"}),
                    },
                    json!({"vendor_call_id": "vc_1"}),
                ),
                event_with_raw(
                    AgentEventKind::ToolResult {
                        tool_name: "bash".into(),
                        tool_use_id: Some("c1".into()),
                        output: json!("output"),
                        is_error: false,
                    },
                    json!({"duration_ms": 42}),
                ),
                event_with_raw(
                    AgentEventKind::RunCompleted {
                        message: "done".into(),
                    },
                    json!({"total_cost": 0.001}),
                ),
            ];
            let receipt = passthrough_receipt_default(events);
            assert_eq!(receipt.trace.len(), 5);

            // All ext values present after receipt construction.
            assert_eq!(
                receipt.trace[0].ext.as_ref().unwrap()["raw_message"]["vendor_trace_id"],
                "trace_001"
            );
            assert_eq!(
                receipt.trace[1].ext.as_ref().unwrap()["raw_message"]["chunk_index"],
                0
            );
            assert_eq!(
                receipt.trace[2].ext.as_ref().unwrap()["raw_message"]["vendor_call_id"],
                "vc_1"
            );
            assert_eq!(
                receipt.trace[3].ext.as_ref().unwrap()["raw_message"]["duration_ms"],
                42
            );
            assert_eq!(
                receipt.trace[4].ext.as_ref().unwrap()["raw_message"]["total_cost"],
                json!(0.001)
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 5. Cross-dialect consistency in passthrough
    // ═══════════════════════════════════════════════════════════════════

    mod cross_dialect_passthrough {
        use super::*;

        #[test]
        fn all_six_dialects_known() {
            let all = Dialect::all();
            assert_eq!(all.len(), 6);
            assert!(all.contains(&Dialect::OpenAi));
            assert!(all.contains(&Dialect::Claude));
            assert!(all.contains(&Dialect::Gemini));
            assert!(all.contains(&Dialect::Codex));
            assert!(all.contains(&Dialect::Kimi));
            assert!(all.contains(&Dialect::Copilot));
        }

        #[test]
        fn dialect_labels_preserved_through_serde() {
            for dialect in Dialect::all() {
                let json_str = serde_json::to_string(dialect).unwrap();
                let back: Dialect = serde_json::from_str(&json_str).unwrap();
                assert_eq!(*dialect, back);
            }
        }

        #[test]
        fn each_dialect_raw_payload_survives_receipt_roundtrip() {
            let dialect_payloads = vec![
                (
                    "openai",
                    json!({"id": "chatcmpl-1", "model": "gpt-4o", "choices": []}),
                ),
                (
                    "claude",
                    json!({"id": "msg_1", "model": "claude-sonnet-4-20250514", "content": []}),
                ),
                (
                    "gemini",
                    json!({"candidates": [], "modelVersion": "gemini-2.5-flash"}),
                ),
                (
                    "codex",
                    json!({"id": "resp_1", "model": "codex-mini-latest", "output": []}),
                ),
                (
                    "kimi",
                    json!({"id": "cmpl-1", "model": "moonshot-v1-128k", "choices": []}),
                ),
                (
                    "copilot",
                    json!({"message": "ok", "references": [], "copilot_errors": []}),
                ),
            ];

            for (dialect_name, payload) in &dialect_payloads {
                let event = event_with_raw(
                    AgentEventKind::AssistantMessage {
                        text: "test".into(),
                    },
                    payload.clone(),
                );
                let receipt = passthrough_receipt_default(vec![event]);
                let json_str = serde_json::to_string(&receipt).unwrap();
                let back: Receipt = serde_json::from_str(&json_str).unwrap();
                let stored = back.trace[0]
                    .ext
                    .as_ref()
                    .unwrap()
                    .get("raw_message")
                    .unwrap();
                assert_eq!(
                    stored, payload,
                    "dialect '{dialect_name}' raw payload mismatch after receipt roundtrip"
                );
            }
        }

        #[test]
        fn passthrough_mode_consistent_across_all_dialects() {
            for dialect in Dialect::all() {
                let receipt = ReceiptBuilder::new(format!("passthrough-{}", dialect.label()))
                    .mode(ExecutionMode::Passthrough)
                    .build();
                assert_eq!(
                    receipt.mode,
                    ExecutionMode::Passthrough,
                    "dialect {} should be passthrough",
                    dialect.label()
                );
                let json_str = serde_json::to_string(&receipt).unwrap();
                let val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
                assert_eq!(
                    val["mode"],
                    json!("passthrough"),
                    "dialect {} mode mismatch in JSON",
                    dialect.label()
                );
            }
        }

        #[test]
        fn receipt_hash_identical_for_same_passthrough_content() {
            let events = vec![assistant_event("Hello")];
            let now = Utc::now();
            let run_id = Uuid::nil();
            let wo_id = Uuid::nil();

            let make_receipt = || Receipt {
                meta: RunMetadata {
                    run_id,
                    work_order_id: wo_id,
                    contract_version: CONTRACT_VERSION.to_string(),
                    started_at: now,
                    finished_at: now,
                    duration_ms: 0,
                },
                backend: BackendIdentity {
                    id: "mock".into(),
                    backend_version: None,
                    adapter_version: None,
                },
                capabilities: CapabilityManifest::new(),
                mode: ExecutionMode::Passthrough,
                usage_raw: serde_json::Value::Null,
                usage: UsageNormalized::default(),
                trace: vec![AgentEvent {
                    ts: now,
                    kind: AgentEventKind::AssistantMessage {
                        text: "Hello".into(),
                    },
                    ext: None,
                }],
                artifacts: vec![],
                verification: VerificationReport::default(),
                outcome: Outcome::Complete,
                receipt_sha256: None,
            };

            let r1 = make_receipt().with_hash().unwrap();
            let r2 = make_receipt().with_hash().unwrap();
            assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
        }

        #[test]
        fn passthrough_vs_mapped_receipt_hashes_differ() {
            let now = Utc::now();
            let run_id = Uuid::nil();
            let wo_id = Uuid::nil();

            let make_receipt = |mode: ExecutionMode| Receipt {
                meta: RunMetadata {
                    run_id,
                    work_order_id: wo_id,
                    contract_version: CONTRACT_VERSION.to_string(),
                    started_at: now,
                    finished_at: now,
                    duration_ms: 0,
                },
                backend: BackendIdentity {
                    id: "mock".into(),
                    backend_version: None,
                    adapter_version: None,
                },
                capabilities: CapabilityManifest::new(),
                mode,
                usage_raw: serde_json::Value::Null,
                usage: UsageNormalized::default(),
                trace: vec![AgentEvent {
                    ts: now,
                    kind: AgentEventKind::AssistantMessage {
                        text: "Hello".into(),
                    },
                    ext: None,
                }],
                artifacts: vec![],
                verification: VerificationReport::default(),
                outcome: Outcome::Complete,
                receipt_sha256: None,
            };

            let pt = make_receipt(ExecutionMode::Passthrough)
                .with_hash()
                .unwrap();
            let mp = make_receipt(ExecutionMode::Mapped).with_hash().unwrap();
            assert_ne!(pt.receipt_sha256, mp.receipt_sha256);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 6. Envelope-level passthrough integration
    // ═══════════════════════════════════════════════════════════════════

    mod envelope_passthrough {
        use super::*;

        #[test]
        fn hello_envelope_passthrough_mode_preserved() {
            let hello = Envelope::Hello {
                contract_version: CONTRACT_VERSION.into(),
                backend: BackendIdentity {
                    id: "test".into(),
                    backend_version: None,
                    adapter_version: None,
                },
                capabilities: CapabilityManifest::new(),
                mode: ExecutionMode::Passthrough,
            };
            let json_str = serde_json::to_string(&hello).unwrap();
            let back: Envelope = serde_json::from_str(&json_str).unwrap();
            match &back {
                Envelope::Hello { mode, .. } => {
                    assert_eq!(*mode, ExecutionMode::Passthrough);
                }
                other => panic!("expected Hello, got {other:?}"),
            }
        }

        #[test]
        fn event_envelope_preserves_event_ext() {
            let mut ext = BTreeMap::new();
            ext.insert(
                "raw_message".to_string(),
                json!({"vendor": "openai", "id": "abc"}),
            );
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "test".into(),
                },
                ext: Some(ext),
            };
            let envelope = Envelope::Event {
                ref_id: "run_1".into(),
                event,
            };
            let json_str = serde_json::to_string(&envelope).unwrap();
            let back: Envelope = serde_json::from_str(&json_str).unwrap();
            match &back {
                Envelope::Event { event, .. } => {
                    let raw = event.ext.as_ref().unwrap().get("raw_message").unwrap();
                    assert_eq!(raw["vendor"], "openai");
                    assert_eq!(raw["id"], "abc");
                }
                other => panic!("expected Event, got {other:?}"),
            }
        }

        #[test]
        fn final_envelope_preserves_passthrough_receipt() {
            let receipt = passthrough_receipt_default(vec![
                assistant_event("Hello"),
                tool_call_event("bash", "c1", json!({"cmd": "ls"})),
            ]);
            let envelope = Envelope::Final {
                ref_id: "run_2".into(),
                receipt,
            };
            let json_str = serde_json::to_string(&envelope).unwrap();
            let back: Envelope = serde_json::from_str(&json_str).unwrap();
            match &back {
                Envelope::Final { receipt, .. } => {
                    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
                    assert_eq!(receipt.trace.len(), 2);
                }
                other => panic!("expected Final, got {other:?}"),
            }
        }

        #[test]
        fn run_envelope_preserves_work_order_config() {
            let wo = WorkOrderBuilder::new("Refactor auth")
                .model("claude-sonnet-4-20250514")
                .max_turns(5)
                .max_budget_usd(1.0)
                .build();
            let envelope = Envelope::Run {
                id: "run_3".into(),
                work_order: wo,
            };
            let json_str = serde_json::to_string(&envelope).unwrap();
            let back: Envelope = serde_json::from_str(&json_str).unwrap();
            match &back {
                Envelope::Run { work_order, .. } => {
                    assert_eq!(
                        work_order.config.model.as_deref(),
                        Some("claude-sonnet-4-20250514")
                    );
                    assert_eq!(work_order.config.max_turns, Some(5));
                    assert_eq!(work_order.config.max_budget_usd, Some(1.0));
                }
                other => panic!("expected Run, got {other:?}"),
            }
        }
    }
}
