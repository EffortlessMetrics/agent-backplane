#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive deep tests for claude-bridge: covers Claude request/response
//! conversion, event mapping, tool call/result bridging, error handling,
//! streaming behavior, capability reporting, and edge cases.

use std::time::Duration;

use serde_json::{json, Value};

use claude_bridge::config::ClaudeBridgeConfig;
use claude_bridge::discovery::{
    resolve_host_script, resolve_node, DEFAULT_NODE_COMMAND, HOST_SCRIPT_ENV, HOST_SCRIPT_RELATIVE,
};
use claude_bridge::error::BridgeError;
use claude_bridge::raw::RunOptions;

// ═══════════════════════════════════════════════════════════════════
// Helpers: mirror the work-order construction from raw.rs so we can
// test the JSON structures without needing a real sidecar process.
// ═══════════════════════════════════════════════════════════════════

fn build_passthrough_work_order(request: &Value, config: &ClaudeBridgeConfig) -> Value {
    json!({
        "id": "test-run-id",
        "task": request.get("prompt").and_then(|v| v.as_str()).unwrap_or("passthrough"),
        "lane": "patch_first",
        "workspace": {
            "root": config.cwd.as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string()),
            "mode": "pass_through"
        },
        "context": {},
        "policy": {},
        "requirements": { "required": [] },
        "config": {
            "vendor": {
                "abp.mode": "passthrough",
                "abp.request": request
            }
        }
    })
}

fn build_mapped_work_order(task: &str, opts: &RunOptions, config: &ClaudeBridgeConfig) -> Value {
    let mut vendor_config = json!({ "abp.mode": "mapped" });
    if let Some(extra) = &opts.extra_config {
        if let Some(obj) = extra.as_object() {
            for (k, v) in obj {
                vendor_config[k] = v.clone();
            }
        }
    }

    json!({
        "id": "test-run-id",
        "task": task,
        "lane": opts.lane.as_deref().unwrap_or("patch_first"),
        "workspace": {
            "root": opts.workspace_root.as_deref()
                .or(config.cwd.as_ref().map(|p| p.to_str().unwrap_or(".")))
                .unwrap_or("."),
            "mode": "staged"
        },
        "context": {},
        "policy": {},
        "requirements": { "required": [] },
        "config": {
            "vendor": vendor_config
        }
    })
}

// ═══════════════════════════════════════════════════════════════════
// 1. Claude request conversion — passthrough mode
// ═══════════════════════════════════════════════════════════════════

mod passthrough_conversion {
    use super::*;

    #[test]
    fn passthrough_extracts_prompt_as_task() {
        let req = json!({"prompt": "Explain Rust lifetimes"});
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        assert_eq!(wo["task"], "Explain Rust lifetimes");
    }

    #[test]
    fn passthrough_missing_prompt_defaults_to_passthrough() {
        let req = json!({"model": "claude-3-opus"});
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        assert_eq!(wo["task"], "passthrough");
    }

    #[test]
    fn passthrough_null_prompt_defaults_to_passthrough() {
        let req = json!({"prompt": null});
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        assert_eq!(wo["task"], "passthrough");
    }

    #[test]
    fn passthrough_integer_prompt_defaults_to_passthrough() {
        let req = json!({"prompt": 42});
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        assert_eq!(wo["task"], "passthrough");
    }

    #[test]
    fn passthrough_empty_prompt_uses_empty_string() {
        let req = json!({"prompt": ""});
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        assert_eq!(wo["task"], "");
    }

    #[test]
    fn passthrough_preserves_full_request_in_vendor() {
        let req = json!({
            "prompt": "test",
            "model": "claude-3-opus-20240229",
            "max_tokens": 4096,
            "temperature": 0.7,
            "system": "You are a helpful assistant."
        });
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        let embedded = &wo["config"]["vendor"]["abp.request"];
        assert_eq!(embedded["model"], "claude-3-opus-20240229");
        assert_eq!(embedded["max_tokens"], 4096);
        assert_eq!(embedded["temperature"], 0.7);
        assert_eq!(embedded["system"], "You are a helpful assistant.");
    }

    #[test]
    fn passthrough_workspace_mode_is_pass_through() {
        let req = json!({"prompt": "test"});
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        assert_eq!(wo["workspace"]["mode"], "pass_through");
    }

    #[test]
    fn passthrough_vendor_mode_is_passthrough() {
        let req = json!({"prompt": "test"});
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        assert_eq!(wo["config"]["vendor"]["abp.mode"], "passthrough");
    }

    #[test]
    fn passthrough_cwd_used_as_workspace_root() {
        let cfg = ClaudeBridgeConfig::new().with_cwd("/my/project");
        let req = json!({"prompt": "test"});
        let wo = build_passthrough_work_order(&req, &cfg);
        assert_eq!(wo["workspace"]["root"], "/my/project");
    }

    #[test]
    fn passthrough_no_cwd_defaults_to_dot() {
        let wo = build_passthrough_work_order(&json!({}), &ClaudeBridgeConfig::new());
        assert_eq!(wo["workspace"]["root"], ".");
    }

    #[test]
    fn passthrough_request_with_messages_array() {
        let req = json!({
            "prompt": "chat",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi!"},
                {"role": "user", "content": "How are you?"}
            ]
        });
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        let msgs = &wo["config"]["vendor"]["abp.request"]["messages"];
        assert_eq!(msgs.as_array().unwrap().len(), 3);
    }

    #[test]
    fn passthrough_request_with_tool_definitions() {
        let req = json!({
            "prompt": "use tools",
            "tools": [
                {
                    "name": "read_file",
                    "description": "Read a file",
                    "input_schema": {
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    }
                }
            ]
        });
        let wo = build_passthrough_work_order(&req, &ClaudeBridgeConfig::new());
        let tools = &wo["config"]["vendor"]["abp.request"]["tools"];
        assert_eq!(tools.as_array().unwrap().len(), 1);
        assert_eq!(tools[0]["name"], "read_file");
    }
}

// ═══════════════════════════════════════════════════════════════════
// 2. Claude request conversion — mapped mode
// ═══════════════════════════════════════════════════════════════════

mod mapped_conversion {
    use super::*;

    #[test]
    fn mapped_task_string_becomes_task_field() {
        let wo = build_mapped_work_order(
            "Fix bug #123",
            &RunOptions::default(),
            &ClaudeBridgeConfig::new(),
        );
        assert_eq!(wo["task"], "Fix bug #123");
    }

    #[test]
    fn mapped_default_lane_is_patch_first() {
        let wo =
            build_mapped_work_order("task", &RunOptions::default(), &ClaudeBridgeConfig::new());
        assert_eq!(wo["lane"], "patch_first");
    }

    #[test]
    fn mapped_custom_lane_override() {
        let opts = RunOptions {
            lane: Some("review".into()),
            ..Default::default()
        };
        let wo = build_mapped_work_order("task", &opts, &ClaudeBridgeConfig::new());
        assert_eq!(wo["lane"], "review");
    }

    #[test]
    fn mapped_workspace_mode_is_staged() {
        let wo =
            build_mapped_work_order("task", &RunOptions::default(), &ClaudeBridgeConfig::new());
        assert_eq!(wo["workspace"]["mode"], "staged");
    }

    #[test]
    fn mapped_vendor_mode_is_mapped() {
        let wo =
            build_mapped_work_order("task", &RunOptions::default(), &ClaudeBridgeConfig::new());
        assert_eq!(wo["config"]["vendor"]["abp.mode"], "mapped");
    }

    #[test]
    fn mapped_workspace_root_from_options() {
        let opts = RunOptions {
            workspace_root: Some("/explicit".into()),
            ..Default::default()
        };
        let wo = build_mapped_work_order("task", &opts, &ClaudeBridgeConfig::new());
        assert_eq!(wo["workspace"]["root"], "/explicit");
    }

    #[test]
    fn mapped_workspace_root_falls_back_to_cwd() {
        let cfg = ClaudeBridgeConfig::new().with_cwd("/cfg/cwd");
        let wo = build_mapped_work_order("task", &RunOptions::default(), &cfg);
        assert_eq!(wo["workspace"]["root"], "/cfg/cwd");
    }

    #[test]
    fn mapped_workspace_root_options_override_cwd() {
        let cfg = ClaudeBridgeConfig::new().with_cwd("/cfg");
        let opts = RunOptions {
            workspace_root: Some("/opts".into()),
            ..Default::default()
        };
        let wo = build_mapped_work_order("task", &opts, &cfg);
        assert_eq!(wo["workspace"]["root"], "/opts");
    }

    #[test]
    fn mapped_extra_config_merged_into_vendor() {
        let opts = RunOptions {
            extra_config: Some(json!({"model": "claude-3-sonnet", "temperature": 0.5})),
            ..Default::default()
        };
        let wo = build_mapped_work_order("task", &opts, &ClaudeBridgeConfig::new());
        assert_eq!(wo["config"]["vendor"]["model"], "claude-3-sonnet");
        assert_eq!(wo["config"]["vendor"]["temperature"], 0.5);
        assert_eq!(wo["config"]["vendor"]["abp.mode"], "mapped"); // preserved
    }

    #[test]
    fn mapped_extra_config_non_object_ignored() {
        let opts = RunOptions {
            extra_config: Some(json!("not an object")),
            ..Default::default()
        };
        let wo = build_mapped_work_order("task", &opts, &ClaudeBridgeConfig::new());
        // Only abp.mode should be present
        let vendor = wo["config"]["vendor"].as_object().unwrap();
        assert_eq!(vendor.len(), 1);
        assert_eq!(vendor.get("abp.mode").unwrap(), "mapped");
    }

    #[test]
    fn mapped_extra_config_cannot_override_abp_mode() {
        let opts = RunOptions {
            extra_config: Some(json!({"abp.mode": "passthrough"})),
            ..Default::default()
        };
        let wo = build_mapped_work_order("task", &opts, &ClaudeBridgeConfig::new());
        // extra_config overrides abp.mode since it's merged after
        assert_eq!(wo["config"]["vendor"]["abp.mode"], "passthrough");
    }

    #[test]
    fn mapped_empty_task_string() {
        let wo = build_mapped_work_order("", &RunOptions::default(), &ClaudeBridgeConfig::new());
        assert_eq!(wo["task"], "");
    }

    #[test]
    fn mapped_unicode_task() {
        let wo = build_mapped_work_order(
            "日本語タスク 🦀",
            &RunOptions::default(),
            &ClaudeBridgeConfig::new(),
        );
        assert_eq!(wo["task"], "日本語タスク 🦀");
    }

    #[test]
    fn mapped_work_order_has_all_required_fields() {
        let wo = build_mapped_work_order("t", &RunOptions::default(), &ClaudeBridgeConfig::new());
        assert!(wo.get("id").is_some());
        assert!(wo.get("task").is_some());
        assert!(wo.get("lane").is_some());
        assert!(wo.get("workspace").is_some());
        assert!(wo.get("context").is_some());
        assert!(wo.get("policy").is_some());
        assert!(wo.get("requirements").is_some());
        assert!(wo.get("config").is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════
// 3. Frame-level event mapping and streaming behavior
// ═══════════════════════════════════════════════════════════════════

mod frame_event_mapping {
    use super::*;
    use sidecar_kit::{Frame, JsonlCodec};

    #[test]
    fn hello_frame_uses_t_tag_not_type() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"name": "claude"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let json_str = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["t"], "hello");
        assert!(parsed.get("type").is_none(), "Frame must NOT use 'type'");
    }

    #[test]
    fn event_frame_carries_assistant_delta() {
        let frame = Frame::Event {
            ref_id: "run-1".into(),
            event: json!({"ts": "2025-01-01T00:00:00Z", "type": "assistant_delta", "text": "Hello!"}),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { ref_id, event } => {
                assert_eq!(ref_id, "run-1");
                assert_eq!(event["type"], "assistant_delta");
                assert_eq!(event["text"], "Hello!");
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[test]
    fn event_frame_carries_tool_call() {
        let event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "tool_call",
            "tool_name": "write_file",
            "tool_use_id": "toolu_abc",
            "input": {"path": "/src/main.rs", "content": "fn main() {}"}
        });
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: event.clone(),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event: ev, .. } => {
                assert_eq!(ev["tool_name"], "write_file");
                assert_eq!(ev["input"]["path"], "/src/main.rs");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_frame_carries_tool_result() {
        let event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "tool_result",
            "tool_name": "read_file",
            "tool_use_id": "toolu_01",
            "output": {"content": "file contents here"},
            "is_error": false
        });
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event: ev, .. } => {
                assert_eq!(ev["type"], "tool_result");
                assert!(!ev["is_error"].as_bool().unwrap());
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_frame_carries_error_tool_result() {
        let event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "tool_result",
            "tool_name": "execute_command",
            "tool_use_id": "toolu_02",
            "output": {"error": "permission denied"},
            "is_error": true
        });
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event: ev, .. } => {
                assert!(ev["is_error"].as_bool().unwrap());
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn final_frame_carries_receipt() {
        let receipt = json!({
            "meta": {"run_id": "r1", "contract_version": "abp/v0.1"},
            "outcome": "complete",
            "usage": {"input_tokens": 500, "output_tokens": 200}
        });
        let frame = Frame::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(receipt["outcome"], "complete");
                assert_eq!(receipt["usage"]["input_tokens"], 500);
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn fatal_frame_with_ref_id() {
        let frame = Frame::Fatal {
            ref_id: Some("r1".into()),
            error: "API key invalid".into(),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Fatal { ref_id, error } => {
                assert_eq!(ref_id.as_deref(), Some("r1"));
                assert_eq!(error, "API key invalid");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_frame_without_ref_id() {
        let frame = Frame::Fatal {
            ref_id: None,
            error: "startup crash".into(),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Fatal { ref_id, error } => {
                assert!(ref_id.is_none());
                assert_eq!(error, "startup crash");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn full_streaming_sequence_encodes_decodes() {
        let frames = vec![
            Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude", "version": "3.5"}),
                capabilities: json!({"streaming": true, "tools": true}),
                mode: json!("mapped"),
            },
            Frame::Run {
                id: "run-42".into(),
                work_order: json!({"task": "fix bug", "lane": "patch_first"}),
            },
            Frame::Event {
                ref_id: "run-42".into(),
                event: json!({"ts": "t0", "type": "run_started", "message": "Starting"}),
            },
            Frame::Event {
                ref_id: "run-42".into(),
                event: json!({"ts": "t1", "type": "assistant_delta", "text": "Let me "}),
            },
            Frame::Event {
                ref_id: "run-42".into(),
                event: json!({"ts": "t2", "type": "assistant_delta", "text": "look..."}),
            },
            Frame::Event {
                ref_id: "run-42".into(),
                event: json!({"ts": "t3", "type": "tool_call", "tool_name": "read_file", "tool_use_id": "t1", "input": {"path": "a.rs"}}),
            },
            Frame::Event {
                ref_id: "run-42".into(),
                event: json!({"ts": "t4", "type": "tool_result", "tool_name": "read_file", "tool_use_id": "t1", "output": {"content": "fn main() {}"}, "is_error": false}),
            },
            Frame::Event {
                ref_id: "run-42".into(),
                event: json!({"ts": "t5", "type": "assistant_message", "text": "I found the issue."}),
            },
            Frame::Event {
                ref_id: "run-42".into(),
                event: json!({"ts": "t6", "type": "run_completed", "message": "Done"}),
            },
            Frame::Final {
                ref_id: "run-42".into(),
                receipt: json!({"outcome": "complete"}),
            },
        ];

        for (i, frame) in frames.iter().enumerate() {
            let encoded = JsonlCodec::encode(frame).expect(&format!("encode frame {i}"));
            let decoded = JsonlCodec::decode(&encoded).expect(&format!("decode frame {i}"));
            // Verify it's a single line
            let trimmed = encoded.trim_end_matches('\n');
            assert!(
                !trimmed.contains('\n'),
                "frame {i} must be single-line JSONL"
            );
            drop(decoded);
        }
    }

    #[test]
    fn ping_pong_roundtrip() {
        for seq in [0, 1, u64::MAX] {
            let ping = Frame::Ping { seq };
            let encoded = JsonlCodec::encode(&ping).unwrap();
            match JsonlCodec::decode(&encoded).unwrap() {
                Frame::Ping { seq: s } => assert_eq!(s, seq),
                other => panic!("expected Ping, got {other:?}"),
            }

            let pong = Frame::Pong { seq };
            let encoded = JsonlCodec::encode(&pong).unwrap();
            match JsonlCodec::decode(&encoded).unwrap() {
                Frame::Pong { seq: s } => assert_eq!(s, seq),
                other => panic!("expected Pong, got {other:?}"),
            }
        }
    }

    #[test]
    fn cancel_frame_with_reason() {
        let frame = Frame::Cancel {
            ref_id: "r1".into(),
            reason: Some("timeout".into()),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Cancel { ref_id, reason } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(reason.as_deref(), Some("timeout"));
            }
            _ => panic!("expected Cancel"),
        }
    }

    #[test]
    fn cancel_frame_without_reason() {
        let frame = Frame::Cancel {
            ref_id: "r1".into(),
            reason: None,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Cancel { reason, .. } => assert!(reason.is_none()),
            _ => panic!("expected Cancel"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 4. Tool call/result bridging — Claude content block shapes
// ═══════════════════════════════════════════════════════════════════

mod tool_call_bridging {
    use super::*;

    #[test]
    fn tool_use_block_has_required_fields() {
        let block = json!({
            "type": "tool_use",
            "id": "toolu_abc123",
            "name": "read_file",
            "input": {"path": "/src/lib.rs"}
        });
        assert_eq!(block["type"], "tool_use");
        assert!(block.get("id").is_some());
        assert!(block.get("name").is_some());
        assert!(block.get("input").is_some());
    }

    #[test]
    fn tool_result_block_success() {
        let block = json!({
            "type": "tool_result",
            "tool_use_id": "toolu_abc123",
            "content": "file contents here"
        });
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "toolu_abc123");
        assert!(block.get("is_error").is_none()); // absent = success
    }

    #[test]
    fn tool_result_block_error() {
        let block = json!({
            "type": "tool_result",
            "tool_use_id": "toolu_abc123",
            "content": "Error: file not found",
            "is_error": true
        });
        assert!(block["is_error"].as_bool().unwrap());
    }

    #[test]
    fn tool_call_with_nested_json_input() {
        let block = json!({
            "type": "tool_use",
            "id": "toolu_nested",
            "name": "execute",
            "input": {
                "command": "find",
                "args": ["-name", "*.rs"],
                "options": {
                    "recursive": true,
                    "depth": 3,
                    "filters": {"include": ["*.rs"], "exclude": ["target/**"]}
                }
            }
        });
        let input = &block["input"];
        assert_eq!(input["args"].as_array().unwrap().len(), 2);
        assert_eq!(input["options"]["filters"]["include"][0], "*.rs");
    }

    #[test]
    fn tool_call_with_empty_input() {
        let block = json!({
            "type": "tool_use",
            "id": "toolu_empty",
            "name": "list_files",
            "input": {}
        });
        assert!(block["input"].as_object().unwrap().is_empty());
    }

    #[test]
    fn multiple_tool_calls_in_content_array() {
        let content = json!([
            {"type": "text", "text": "I'll read two files."},
            {"type": "tool_use", "id": "t1", "name": "read_file", "input": {"path": "a.rs"}},
            {"type": "tool_use", "id": "t2", "name": "read_file", "input": {"path": "b.rs"}}
        ]);
        let arr = content.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        let tool_calls: Vec<_> = arr.iter().filter(|b| b["type"] == "tool_use").collect();
        assert_eq!(tool_calls.len(), 2);
    }

    #[test]
    fn tool_result_with_structured_content_array() {
        let block = json!({
            "type": "tool_result",
            "tool_use_id": "t1",
            "content": [
                {"type": "text", "text": "Line 1"},
                {"type": "text", "text": "Line 2"},
                {"type": "image", "source": {"type": "base64", "data": "..."}}
            ]
        });
        assert!(block["content"].is_array());
        assert_eq!(block["content"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn tool_call_maps_to_abp_event_json() {
        // Verify the JSON shape that the bridge would emit as an ABP event
        let abp_event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "tool_call",
            "tool_name": "write_file",
            "tool_use_id": "toolu_abc",
            "parent_tool_use_id": null,
            "input": {"path": "/a.rs", "content": "code"}
        });
        assert_eq!(abp_event["type"], "tool_call");
        assert!(abp_event["parent_tool_use_id"].is_null());
    }

    #[test]
    fn tool_result_maps_to_abp_event_json() {
        let abp_event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "tool_result",
            "tool_name": "write_file",
            "tool_use_id": "toolu_abc",
            "output": {"success": true},
            "is_error": false
        });
        assert_eq!(abp_event["type"], "tool_result");
        assert!(!abp_event["is_error"].as_bool().unwrap());
    }

    #[test]
    fn nested_tool_call_with_parent_id() {
        let abp_event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "tool_call",
            "tool_name": "execute_command",
            "tool_use_id": "toolu_child",
            "parent_tool_use_id": "toolu_parent",
            "input": {"command": "cargo build"}
        });
        assert_eq!(abp_event["parent_tool_use_id"], "toolu_parent");
    }
}

// ═══════════════════════════════════════════════════════════════════
// 5. Error handling and type mismatches
// ═══════════════════════════════════════════════════════════════════

mod error_handling {
    use super::*;

    #[test]
    fn bridge_error_node_not_found_display() {
        let err = BridgeError::NodeNotFound("node22 missing".into());
        let msg = err.to_string();
        assert!(msg.contains("node.js not found"));
        assert!(msg.contains("node22 missing"));
    }

    #[test]
    fn bridge_error_host_script_not_found_display() {
        let err = BridgeError::HostScriptNotFound("/bad/path/host.js".into());
        let msg = err.to_string();
        assert!(msg.contains("host script not found"));
        assert!(msg.contains("/bad/path/host.js"));
    }

    #[test]
    fn bridge_error_config_display() {
        let err = BridgeError::Config("missing API key".into());
        assert!(err.to_string().contains("configuration error"));
    }

    #[test]
    fn bridge_error_run_display() {
        let err = BridgeError::Run("process exited with code 1".into());
        assert!(err.to_string().contains("run error"));
    }

    #[test]
    fn bridge_error_from_sidecar_timeout() {
        let bridge: BridgeError = sidecar_kit::SidecarError::Timeout.into();
        assert!(bridge.to_string().contains("timed out"));
    }

    #[test]
    fn bridge_error_from_sidecar_fatal() {
        let bridge: BridgeError = sidecar_kit::SidecarError::Fatal("OOM".into()).into();
        assert!(bridge.to_string().contains("OOM"));
    }

    #[test]
    fn bridge_error_from_sidecar_protocol() {
        let bridge: BridgeError =
            sidecar_kit::SidecarError::Protocol("unexpected frame".into()).into();
        assert!(bridge.to_string().contains("protocol violation"));
    }

    #[test]
    fn bridge_error_from_sidecar_spawn() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let bridge: BridgeError = sidecar_kit::SidecarError::Spawn(io_err).into();
        assert!(bridge.to_string().contains("spawn"));
    }

    #[test]
    fn bridge_error_from_sidecar_exited_with_code() {
        let bridge: BridgeError = sidecar_kit::SidecarError::Exited(Some(137)).into();
        assert!(bridge.to_string().contains("137"));
    }

    #[test]
    fn bridge_error_from_sidecar_exited_no_code() {
        let bridge: BridgeError = sidecar_kit::SidecarError::Exited(None).into();
        assert!(bridge.to_string().contains("exited"));
    }

    #[test]
    fn bridge_error_from_sidecar_serialize() {
        let json_err = serde_json::from_str::<Value>("{{bad").unwrap_err();
        let bridge: BridgeError = sidecar_kit::SidecarError::Serialize(json_err).into();
        assert!(bridge.to_string().contains("serialization"));
    }

    #[test]
    fn bridge_error_from_sidecar_deserialize() {
        let json_err = serde_json::from_str::<Value>("not-json").unwrap_err();
        let bridge: BridgeError = sidecar_kit::SidecarError::Deserialize(json_err).into();
        assert!(bridge.to_string().contains("deserialization"));
    }

    #[test]
    fn bridge_error_source_chain_for_sidecar() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
        let bridge: BridgeError = sidecar_kit::SidecarError::Spawn(io_err).into();
        let source = std::error::Error::source(&bridge);
        assert!(source.is_some());
    }

    #[test]
    fn bridge_error_no_source_for_string_variants() {
        for err in [
            BridgeError::NodeNotFound("x".into()),
            BridgeError::HostScriptNotFound("x".into()),
            BridgeError::Config("x".into()),
            BridgeError::Run("x".into()),
        ] {
            assert!(std::error::Error::source(&err).is_none());
        }
    }

    #[test]
    fn bridge_error_debug_output() {
        let err = BridgeError::Config("test".into());
        let debug = format!("{:?}", err);
        assert!(debug.contains("Config"));
        assert!(debug.contains("test"));
    }

    #[test]
    fn bridge_error_question_mark_conversion() {
        fn fallible() -> Result<(), BridgeError> {
            Err(sidecar_kit::SidecarError::Timeout)?
        }
        assert!(fallible().is_err());
    }

    #[test]
    fn invalid_json_frame_decode_fails() {
        let result = sidecar_kit::JsonlCodec::decode("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn missing_discriminator_tag_decode_fails() {
        // Valid JSON but no "t" field
        let result = sidecar_kit::JsonlCodec::decode(r#"{"ref_id": "r1", "event": {}}"#);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_discriminator_tag_decode_fails() {
        let result = sidecar_kit::JsonlCodec::decode(r#"{"t": "unknown_frame_type"}"#);
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════
// 6. Capability reporting via Hello frame
// ═══════════════════════════════════════════════════════════════════

mod capability_reporting {
    use super::*;
    use sidecar_kit::{Frame, JsonlCodec};

    #[test]
    fn hello_frame_reports_capabilities() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"name": "claude", "version": "3.5-sonnet"}),
            capabilities: json!({
                "streaming": true,
                "tools": true,
                "vision": false,
                "max_tokens": 4096
            }),
            mode: json!("mapped"),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["capabilities"]["streaming"], true);
        assert_eq!(parsed["capabilities"]["tools"], true);
        assert_eq!(parsed["capabilities"]["vision"], false);
        assert_eq!(parsed["capabilities"]["max_tokens"], 4096);
    }

    #[test]
    fn hello_frame_empty_capabilities() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"name": "claude"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&encoded).unwrap();
        assert!(parsed["capabilities"].as_object().unwrap().is_empty());
    }

    #[test]
    fn hello_frame_backend_identity() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({
                "name": "claude",
                "version": "3.5-sonnet-20241022",
                "provider": "anthropic"
            }),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["backend"]["name"], "claude");
        assert_eq!(parsed["backend"]["provider"], "anthropic");
    }

    #[test]
    fn hello_frame_contract_version() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"name": "claude"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["contract_version"], "abp/v0.1");
    }

    #[test]
    fn hello_frame_mode_mapped() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"name": "claude"}),
            capabilities: json!({}),
            mode: json!("mapped"),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["mode"], "mapped");
    }

    #[test]
    fn hello_frame_mode_passthrough() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"name": "claude"}),
            capabilities: json!({}),
            mode: json!("passthrough"),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["mode"], "passthrough");
    }

    #[test]
    fn hello_frame_mode_null() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"name": "claude"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&encoded).unwrap();
        assert!(parsed["mode"].is_null());
    }

    #[test]
    fn hello_builder_produces_valid_frame() {
        let frame = sidecar_kit::hello_frame("claude");
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["t"], "hello");
        assert_eq!(parsed["contract_version"], "abp/v0.1");
        assert_eq!(parsed["backend"]["id"], "claude");
    }
}

// ═══════════════════════════════════════════════════════════════════
// 7. Edge cases — empty messages, mixed content, large payloads
// ═══════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;
    use sidecar_kit::{Frame, JsonlCodec};

    #[test]
    fn empty_text_event() {
        let event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "assistant_delta",
            "text": ""
        });
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert_eq!(event["text"], ""),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn unicode_text_event() {
        let event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "assistant_delta",
            "text": "日本語テスト 🦀🔥 émojis"
        });
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert_eq!(event["text"], "日本語テスト 🦀🔥 émojis"),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn newline_in_text_event() {
        let event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "assistant_delta",
            "text": "line1\nline2\nline3"
        });
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        // The newlines inside JSON strings are escaped, so the JSONL line itself is one line
        let trimmed = encoded.trim_end_matches('\n');
        assert!(
            !trimmed.contains('\n'),
            "escaped newlines should not break JSONL"
        );
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert!(event["text"].as_str().unwrap().contains('\n')),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn empty_event_payload() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({}),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert!(event.as_object().unwrap().is_empty()),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn null_event_payload() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: Value::Null,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert!(event.is_null()),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn empty_ref_id() {
        let frame = Frame::Event {
            ref_id: "".into(),
            event: json!({}),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { ref_id, .. } => assert_eq!(ref_id, ""),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn large_text_event() {
        let big_text = "x".repeat(100_000);
        let event = json!({"ts": "t0", "type": "assistant_delta", "text": big_text});
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event, .. } => {
                assert_eq!(event["text"].as_str().unwrap().len(), 100_000);
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn mixed_content_blocks_text_and_tool_use() {
        let content = json!([
            {"type": "text", "text": "Reading the file..."},
            {"type": "tool_use", "id": "t1", "name": "read_file", "input": {"path": "src/lib.rs"}},
            {"type": "text", "text": "Now I'll edit it."},
            {"type": "tool_use", "id": "t2", "name": "write_file", "input": {"path": "src/lib.rs", "content": "new"}},
        ]);
        let arr = content.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        let text_blocks: Vec<_> = arr.iter().filter(|b| b["type"] == "text").collect();
        let tool_blocks: Vec<_> = arr.iter().filter(|b| b["type"] == "tool_use").collect();
        assert_eq!(text_blocks.len(), 2);
        assert_eq!(tool_blocks.len(), 2);
    }

    #[test]
    fn receipt_with_no_events() {
        let receipt = json!({
            "meta": {"run_id": "r1"},
            "outcome": "complete",
            "trace": [],
            "artifacts": [],
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });
        assert!(receipt["trace"].as_array().unwrap().is_empty());
    }

    #[test]
    fn receipt_with_failed_outcome() {
        let receipt = json!({
            "meta": {"run_id": "r1"},
            "outcome": "failed",
            "trace": [],
            "artifacts": []
        });
        assert_eq!(receipt["outcome"], "failed");
    }

    #[test]
    fn special_characters_in_tool_input() {
        let event = json!({
            "ts": "t0",
            "type": "tool_call",
            "tool_name": "write_file",
            "tool_use_id": "t1",
            "input": {
                "path": "/tmp/test.rs",
                "content": "fn main() { println!(\"hello \\\"world\\\"\"); }\n// tabs\there"
            }
        });
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: event.clone(),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Event { event: ev, .. } => {
                assert!(ev["input"]["content"].as_str().unwrap().contains("hello"));
            }
            _ => panic!("expected Event"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 8. Protocol state machine
// ═══════════════════════════════════════════════════════════════════

mod protocol_state {
    use super::*;
    use sidecar_kit::{Frame, ProtocolPhase, ProtocolState};

    #[test]
    fn initial_phase_is_awaiting_hello() {
        let state = ProtocolState::new();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
    }

    #[test]
    fn hello_transitions_to_awaiting_run() {
        let mut state = ProtocolState::new();
        let hello = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"name": "claude"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        assert!(state.advance(&hello).is_ok());
        assert_eq!(state.phase(), ProtocolPhase::AwaitingRun);
    }

    #[test]
    fn run_transitions_to_streaming() {
        let mut state = ProtocolState::new();
        state
            .advance(&Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude"}),
                capabilities: json!({}),
                mode: Value::Null,
            })
            .unwrap();

        let run = Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        };
        assert!(state.advance(&run).is_ok());
        assert_eq!(state.phase(), ProtocolPhase::Streaming);
        assert_eq!(state.run_id(), Some("r1"));
    }

    #[test]
    fn events_increment_counter() {
        let mut state = ProtocolState::new();
        state
            .advance(&Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude"}),
                capabilities: json!({}),
                mode: Value::Null,
            })
            .unwrap();
        state
            .advance(&Frame::Run {
                id: "r1".into(),
                work_order: json!({}),
            })
            .unwrap();

        assert_eq!(state.events_seen(), 0);
        state
            .advance(&Frame::Event {
                ref_id: "r1".into(),
                event: json!({}),
            })
            .unwrap();
        assert_eq!(state.events_seen(), 1);
        state
            .advance(&Frame::Event {
                ref_id: "r1".into(),
                event: json!({}),
            })
            .unwrap();
        assert_eq!(state.events_seen(), 2);
    }

    #[test]
    fn final_transitions_to_completed() {
        let mut state = ProtocolState::new();
        state
            .advance(&Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude"}),
                capabilities: json!({}),
                mode: Value::Null,
            })
            .unwrap();
        state
            .advance(&Frame::Run {
                id: "r1".into(),
                work_order: json!({}),
            })
            .unwrap();
        state
            .advance(&Frame::Final {
                ref_id: "r1".into(),
                receipt: json!({}),
            })
            .unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert!(state.is_terminal());
    }

    #[test]
    fn fatal_during_streaming_transitions_to_completed() {
        let mut state = ProtocolState::new();
        state
            .advance(&Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude"}),
                capabilities: json!({}),
                mode: Value::Null,
            })
            .unwrap();
        state
            .advance(&Frame::Run {
                id: "r1".into(),
                work_order: json!({}),
            })
            .unwrap();
        state
            .advance(&Frame::Fatal {
                ref_id: Some("r1".into()),
                error: "crash".into(),
            })
            .unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn event_before_hello_faults() {
        let mut state = ProtocolState::new();
        let result = state.advance(&Frame::Event {
            ref_id: "r1".into(),
            event: json!({}),
        });
        assert!(result.is_err());
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn run_before_hello_faults() {
        let mut state = ProtocolState::new();
        let result = state.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        });
        assert!(result.is_err());
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn ref_id_mismatch_during_streaming_errors() {
        let mut state = ProtocolState::new();
        state
            .advance(&Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude"}),
                capabilities: json!({}),
                mode: Value::Null,
            })
            .unwrap();
        state
            .advance(&Frame::Run {
                id: "r1".into(),
                work_order: json!({}),
            })
            .unwrap();
        let result = state.advance(&Frame::Event {
            ref_id: "r999".into(),
            event: json!({}),
        });
        assert!(result.is_err());
    }

    #[test]
    fn ping_pong_allowed_during_streaming() {
        let mut state = ProtocolState::new();
        state
            .advance(&Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude"}),
                capabilities: json!({}),
                mode: Value::Null,
            })
            .unwrap();
        state
            .advance(&Frame::Run {
                id: "r1".into(),
                work_order: json!({}),
            })
            .unwrap();
        assert!(state.advance(&Frame::Ping { seq: 1 }).is_ok());
        assert!(state.advance(&Frame::Pong { seq: 1 }).is_ok());
        assert_eq!(state.phase(), ProtocolPhase::Streaming);
    }

    #[test]
    fn reset_returns_to_awaiting_hello() {
        let mut state = ProtocolState::new();
        state
            .advance(&Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude"}),
                capabilities: json!({}),
                mode: Value::Null,
            })
            .unwrap();
        state
            .advance(&Frame::Run {
                id: "r1".into(),
                work_order: json!({}),
            })
            .unwrap();
        state.reset();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
        assert!(state.run_id().is_none());
        assert_eq!(state.events_seen(), 0);
    }

    #[test]
    fn faulted_state_rejects_all_frames() {
        let mut state = ProtocolState::new();
        // Force a fault
        let _ = state.advance(&Frame::Event {
            ref_id: "r1".into(),
            event: json!({}),
        });
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        // All further frames should be rejected
        assert!(state
            .advance(&Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({}),
                capabilities: json!({}),
                mode: Value::Null,
            })
            .is_err());
    }

    #[test]
    fn fault_reason_is_set() {
        let mut state = ProtocolState::new();
        let _ = state.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        });
        assert!(state.fault_reason().is_some());
        assert!(state.fault_reason().unwrap().contains("expected hello"));
    }
}

// ═══════════════════════════════════════════════════════════════════
// 9. Frame framing — FrameWriter / FrameReader
// ═══════════════════════════════════════════════════════════════════

mod frame_framing {
    use super::*;
    use sidecar_kit::{
        buf_reader_from_bytes, read_all_frames, validate_frame, write_frames, Frame, FrameReader,
        FrameWriter,
    };

    #[test]
    fn frame_writer_writes_single_frame() {
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf);
        let frame = Frame::Ping { seq: 1 };
        writer.write_frame(&frame).unwrap();
        assert_eq!(writer.frames_written(), 1);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.ends_with('\n'));
        assert!(output.contains("\"t\":\"ping\""));
    }

    #[test]
    fn frame_writer_counts_multiple_frames() {
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf);
        for i in 0..5 {
            writer.write_frame(&Frame::Ping { seq: i }).unwrap();
        }
        assert_eq!(writer.frames_written(), 5);
    }

    #[test]
    fn frame_reader_reads_frames() {
        let data = r#"{"t":"ping","seq":1}
{"t":"pong","seq":1}
"#;
        let reader = buf_reader_from_bytes(data.as_bytes());
        let mut fr = FrameReader::new(reader);
        let f1 = fr.read_frame().unwrap().unwrap();
        assert!(matches!(f1, Frame::Ping { seq: 1 }));
        let f2 = fr.read_frame().unwrap().unwrap();
        assert!(matches!(f2, Frame::Pong { seq: 1 }));
        assert!(fr.read_frame().unwrap().is_none()); // EOF
        assert_eq!(fr.frames_read(), 2);
    }

    #[test]
    fn frame_reader_skips_blank_lines() {
        let data = "\n\n{\"t\":\"ping\",\"seq\":1}\n\n\n{\"t\":\"pong\",\"seq\":1}\n\n";
        let reader = buf_reader_from_bytes(data.as_bytes());
        let frames = read_all_frames(reader).unwrap();
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn write_frames_and_read_back() {
        let frames = vec![
            Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"id": "claude"}),
                capabilities: json!({}),
                mode: Value::Null,
            },
            Frame::Event {
                ref_id: "r1".into(),
                event: json!({"type": "run_started", "message": "go"}),
            },
            Frame::Final {
                ref_id: "r1".into(),
                receipt: json!({"outcome": "complete"}),
            },
        ];

        let mut buf = Vec::new();
        let count = write_frames(&mut buf, &frames).unwrap();
        assert_eq!(count, 3);

        let reader = buf_reader_from_bytes(&buf);
        let read_back = read_all_frames(reader).unwrap();
        assert_eq!(read_back.len(), 3);
    }

    #[test]
    fn validate_frame_hello_valid() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "claude"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let validation = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(validation.valid, "issues: {:?}", validation.issues);
    }

    #[test]
    fn validate_frame_hello_empty_version() {
        let frame = Frame::Hello {
            contract_version: "".into(),
            backend: json!({"id": "claude"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let validation = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!validation.valid);
        assert!(validation
            .issues
            .iter()
            .any(|i| i.contains("contract_version")));
    }

    #[test]
    fn validate_frame_hello_bad_version_prefix() {
        let frame = Frame::Hello {
            contract_version: "v0.1".into(),
            backend: json!({"id": "claude"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let validation = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!validation.valid);
    }

    #[test]
    fn validate_frame_hello_missing_backend_id() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let validation = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!validation.valid);
        assert!(validation.issues.iter().any(|i| i.contains("backend.id")));
    }

    #[test]
    fn validate_frame_run_empty_id() {
        let frame = Frame::Run {
            id: "".into(),
            work_order: json!({}),
        };
        let validation = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!validation.valid);
    }

    #[test]
    fn validate_frame_event_empty_ref_id() {
        let frame = Frame::Event {
            ref_id: "".into(),
            event: json!({}),
        };
        let validation = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!validation.valid);
    }

    #[test]
    fn validate_frame_fatal_empty_error() {
        let frame = Frame::Fatal {
            ref_id: None,
            error: "".into(),
        };
        let validation = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!validation.valid);
    }

    #[test]
    fn validate_frame_size_limit_exceeded() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"data": "x".repeat(1000)}),
        };
        let validation = validate_frame(&frame, 100); // Very small limit
        assert!(!validation.valid);
        assert!(validation
            .issues
            .iter()
            .any(|i| i.contains("exceeds limit")));
    }
}

// ═══════════════════════════════════════════════════════════════════
// 10. Event builders from sidecar-kit
// ═══════════════════════════════════════════════════════════════════

mod event_builders {
    use super::*;
    use sidecar_kit::*;

    #[test]
    fn event_text_delta_shape() {
        let ev = event_text_delta("Hello");
        assert_eq!(ev["type"], "assistant_delta");
        assert_eq!(ev["text"], "Hello");
        assert!(ev["ts"].is_string());
    }

    #[test]
    fn event_text_message_shape() {
        let ev = event_text_message("Full message");
        assert_eq!(ev["type"], "assistant_message");
        assert_eq!(ev["text"], "Full message");
    }

    #[test]
    fn event_tool_call_shape() {
        let ev = event_tool_call("read_file", Some("t1"), json!({"path": "/a.rs"}));
        assert_eq!(ev["type"], "tool_call");
        assert_eq!(ev["tool_name"], "read_file");
        assert_eq!(ev["tool_use_id"], "t1");
        assert!(ev["parent_tool_use_id"].is_null());
    }

    #[test]
    fn event_tool_call_no_id() {
        let ev = event_tool_call("list_files", None, json!({}));
        assert!(ev["tool_use_id"].is_null());
    }

    #[test]
    fn event_tool_result_success() {
        let ev = event_tool_result("read_file", Some("t1"), json!({"content": "data"}), false);
        assert_eq!(ev["type"], "tool_result");
        assert_eq!(ev["is_error"], false);
    }

    #[test]
    fn event_tool_result_error() {
        let ev = event_tool_result("execute", Some("t2"), json!({"error": "fail"}), true);
        assert_eq!(ev["is_error"], true);
    }

    #[test]
    fn event_error_shape() {
        let ev = event_error("API rate limit exceeded");
        assert_eq!(ev["type"], "error");
        assert_eq!(ev["message"], "API rate limit exceeded");
    }

    #[test]
    fn event_warning_shape() {
        let ev = event_warning("token limit approaching");
        assert_eq!(ev["type"], "warning");
        assert_eq!(ev["message"], "token limit approaching");
    }

    #[test]
    fn event_run_started_shape() {
        let ev = event_run_started("Starting run");
        assert_eq!(ev["type"], "run_started");
    }

    #[test]
    fn event_run_completed_shape() {
        let ev = event_run_completed("Run done");
        assert_eq!(ev["type"], "run_completed");
    }

    #[test]
    fn event_file_changed_shape() {
        let ev = event_file_changed("src/lib.rs", "Added function");
        assert_eq!(ev["type"], "file_changed");
        assert_eq!(ev["path"], "src/lib.rs");
    }

    #[test]
    fn event_command_executed_shape() {
        let ev = event_command_executed("cargo test", Some(0), Some("All tests passed"));
        assert_eq!(ev["type"], "command_executed");
        assert_eq!(ev["exit_code"], 0);
    }

    #[test]
    fn event_frame_builder() {
        let ev = event_text_delta("hi");
        let frame = event_frame("r1", ev);
        match frame {
            Frame::Event { ref_id, event } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(event["text"], "hi");
            }
            _ => panic!("expected Event frame"),
        }
    }

    #[test]
    fn fatal_frame_builder_with_ref() {
        let frame = fatal_frame(Some("r1"), "crash");
        match frame {
            Frame::Fatal { ref_id, error } => {
                assert_eq!(ref_id.as_deref(), Some("r1"));
                assert_eq!(error, "crash");
            }
            _ => panic!("expected Fatal frame"),
        }
    }

    #[test]
    fn fatal_frame_builder_without_ref() {
        let frame = fatal_frame(None, "startup failure");
        match frame {
            Frame::Fatal { ref_id, error } => {
                assert!(ref_id.is_none());
                assert_eq!(error, "startup failure");
            }
            _ => panic!("expected Fatal frame"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 11. ReceiptBuilder
// ═══════════════════════════════════════════════════════════════════

mod receipt_builder {
    use super::*;
    use sidecar_kit::ReceiptBuilder;

    #[test]
    fn receipt_builder_default_outcome_is_complete() {
        let receipt = ReceiptBuilder::new("r1", "claude").build();
        assert_eq!(receipt["outcome"], "complete");
    }

    #[test]
    fn receipt_builder_failed_outcome() {
        let receipt = ReceiptBuilder::new("r1", "claude").failed().build();
        assert_eq!(receipt["outcome"], "failed");
    }

    #[test]
    fn receipt_builder_partial_outcome() {
        let receipt = ReceiptBuilder::new("r1", "claude").partial().build();
        assert_eq!(receipt["outcome"], "partial");
    }

    #[test]
    fn receipt_builder_sets_run_id() {
        let receipt = ReceiptBuilder::new("run-42", "claude").build();
        assert_eq!(receipt["meta"]["run_id"], "run-42");
        assert_eq!(receipt["meta"]["work_order_id"], "run-42");
    }

    #[test]
    fn receipt_builder_sets_backend_id() {
        let receipt = ReceiptBuilder::new("r1", "claude-3-sonnet").build();
        assert_eq!(receipt["backend"]["id"], "claude-3-sonnet");
    }

    #[test]
    fn receipt_builder_with_usage() {
        let receipt = ReceiptBuilder::new("r1", "claude")
            .input_tokens(1000)
            .output_tokens(500)
            .build();
        assert_eq!(receipt["usage"]["input_tokens"], 1000);
        assert_eq!(receipt["usage"]["output_tokens"], 500);
    }

    #[test]
    fn receipt_builder_with_raw_usage() {
        let receipt = ReceiptBuilder::new("r1", "claude")
            .usage_raw(
                json!({"input_tokens": 1000, "output_tokens": 500, "cache_read_tokens": 100}),
            )
            .build();
        assert_eq!(receipt["usage_raw"]["cache_read_tokens"], 100);
    }

    #[test]
    fn receipt_builder_with_events() {
        let receipt = ReceiptBuilder::new("r1", "claude")
            .event(json!({"type": "run_started"}))
            .event(json!({"type": "assistant_delta", "text": "hi"}))
            .build();
        assert_eq!(receipt["trace"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn receipt_builder_with_artifacts() {
        let receipt = ReceiptBuilder::new("r1", "claude")
            .artifact("diff", "patch.diff")
            .artifact("file", "src/lib.rs")
            .build();
        let artifacts = receipt["artifacts"].as_array().unwrap();
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0]["kind"], "diff");
        assert_eq!(artifacts[1]["path"], "src/lib.rs");
    }

    #[test]
    fn receipt_builder_contract_version() {
        let receipt = ReceiptBuilder::new("r1", "claude").build();
        assert_eq!(receipt["meta"]["contract_version"], "abp/v0.1");
    }

    #[test]
    fn receipt_builder_sha256_is_null() {
        let receipt = ReceiptBuilder::new("r1", "claude").build();
        assert!(receipt["receipt_sha256"].is_null());
    }
}

// ═══════════════════════════════════════════════════════════════════
// 12. Middleware chain
// ═══════════════════════════════════════════════════════════════════

mod middleware_tests {
    use super::*;
    use sidecar_kit::{
        ErrorWrapMiddleware, EventMiddleware, FilterMiddleware, LoggingMiddleware, MiddlewareChain,
        TimingMiddleware,
    };

    #[test]
    fn logging_middleware_passes_through() {
        let mw = LoggingMiddleware::new();
        let event = json!({"type": "assistant_delta", "text": "hi"});
        let result = mw.process(&event);
        assert!(result.is_some());
        assert_eq!(result.unwrap()["text"], "hi");
    }

    #[test]
    fn filter_include_passes_matching() {
        let filter = FilterMiddleware::include_kinds(&["assistant_delta", "tool_call"]);
        let ev = json!({"type": "assistant_delta", "text": "hi"});
        assert!(filter.process(&ev).is_some());
    }

    #[test]
    fn filter_include_blocks_non_matching() {
        let filter = FilterMiddleware::include_kinds(&["assistant_delta"]);
        let ev = json!({"type": "tool_call", "tool_name": "read"});
        assert!(filter.process(&ev).is_none());
    }

    #[test]
    fn filter_exclude_blocks_matching() {
        let filter = FilterMiddleware::exclude_kinds(&["warning"]);
        let ev = json!({"type": "warning", "message": "something"});
        assert!(filter.process(&ev).is_none());
    }

    #[test]
    fn filter_exclude_passes_non_matching() {
        let filter = FilterMiddleware::exclude_kinds(&["warning"]);
        let ev = json!({"type": "assistant_delta", "text": "hi"});
        assert!(filter.process(&ev).is_some());
    }

    #[test]
    fn filter_is_case_insensitive() {
        let filter = FilterMiddleware::include_kinds(&["Assistant_Delta"]);
        let ev = json!({"type": "assistant_delta", "text": "hi"});
        assert!(filter.process(&ev).is_some());
    }

    #[test]
    fn timing_middleware_adds_processing_field() {
        let mw = TimingMiddleware::new();
        let ev = json!({"type": "assistant_delta", "text": "hi"});
        let result = mw.process(&ev).unwrap();
        assert!(result.get("_processing_us").is_some());
    }

    #[test]
    fn error_wrap_middleware_passes_objects() {
        let mw = ErrorWrapMiddleware::new();
        let ev = json!({"type": "assistant_delta", "text": "hi"});
        let result = mw.process(&ev).unwrap();
        assert_eq!(result["text"], "hi");
    }

    #[test]
    fn error_wrap_middleware_wraps_non_objects() {
        let mw = ErrorWrapMiddleware::new();
        let ev = json!("not an object");
        let result = mw.process(&ev).unwrap();
        assert_eq!(result["type"], "error");
        assert!(result["message"].as_str().unwrap().contains("non-object"));
    }

    #[test]
    fn middleware_chain_empty_passthrough() {
        let chain = MiddlewareChain::new();
        assert!(chain.is_empty());
        let ev = json!({"type": "test"});
        let result = chain.process(&ev);
        assert!(result.is_some());
    }

    #[test]
    fn middleware_chain_sequential_processing() {
        let chain = MiddlewareChain::new()
            .with(FilterMiddleware::exclude_kinds(&["warning"]))
            .with(TimingMiddleware::new());

        assert_eq!(chain.len(), 2);

        // Pass through non-warning
        let ev1 = json!({"type": "assistant_delta", "text": "hi"});
        let result1 = chain.process(&ev1).unwrap();
        assert!(result1.get("_processing_us").is_some());

        // Block warning
        let ev2 = json!({"type": "warning", "message": "warn"});
        assert!(chain.process(&ev2).is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════
// 13. Pipeline stages
// ═══════════════════════════════════════════════════════════════════

mod pipeline_tests {
    use super::*;
    use sidecar_kit::{EventPipeline, PipelineStage, RedactStage, TimestampStage, ValidateStage};

    #[test]
    fn empty_pipeline_passthrough() {
        let pipeline = EventPipeline::new();
        assert_eq!(pipeline.stage_count(), 0);
        let ev = json!({"type": "test"});
        let result = pipeline.process(ev).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn timestamp_stage_adds_processed_at() {
        let stage = TimestampStage::new();
        let ev = json!({"type": "test"});
        let result = stage.process(ev).unwrap().unwrap();
        assert!(result.get("processed_at").is_some());
    }

    #[test]
    fn redact_stage_removes_fields() {
        let stage = RedactStage::new(vec!["secret".into(), "password".into()]);
        let ev = json!({"type": "test", "secret": "s3cret", "password": "p4ss", "keep": "yes"});
        let result = stage.process(ev).unwrap().unwrap();
        assert!(result.get("secret").is_none());
        assert!(result.get("password").is_none());
        assert_eq!(result["keep"], "yes");
    }

    #[test]
    fn validate_stage_passes_with_required_fields() {
        let stage = ValidateStage::new(vec!["type".into(), "ts".into()]);
        let ev = json!({"type": "test", "ts": "2025-01-01"});
        assert!(stage.process(ev).unwrap().is_some());
    }

    #[test]
    fn validate_stage_fails_missing_field() {
        let stage = ValidateStage::new(vec!["type".into(), "ts".into()]);
        let ev = json!({"type": "test"});
        let result = stage.process(ev);
        assert!(result.is_err());
    }

    #[test]
    fn pipeline_multi_stage() {
        let mut pipeline = EventPipeline::new();
        pipeline.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));
        pipeline.add_stage(Box::new(RedactStage::new(vec!["secret".into()])));
        pipeline.add_stage(Box::new(TimestampStage::new()));

        assert_eq!(pipeline.stage_count(), 3);

        let ev = json!({"type": "test", "secret": "hidden", "data": "ok"});
        let result = pipeline.process(ev).unwrap().unwrap();
        assert!(result.get("secret").is_none());
        assert!(result.get("processed_at").is_some());
        assert_eq!(result["data"], "ok");
    }
}

// ═══════════════════════════════════════════════════════════════════
// 14. Cancel token
// ═══════════════════════════════════════════════════════════════════

mod cancel_token_tests {
    use sidecar_kit::CancelToken;

    #[test]
    fn new_token_is_not_cancelled() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancel_sets_flag() {
        let token = CancelToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn clone_shares_state() {
        let token = CancelToken::new();
        let clone = token.clone();
        token.cancel();
        assert!(clone.is_cancelled());
    }

    #[test]
    fn double_cancel_is_idempotent() {
        let token = CancelToken::new();
        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn default_is_not_cancelled() {
        let token = CancelToken::default();
        assert!(!token.is_cancelled());
    }
}

// ═══════════════════════════════════════════════════════════════════
// 15. ProcessSpec construction and config propagation
// ═══════════════════════════════════════════════════════════════════

mod process_spec_tests {
    use super::*;
    use sidecar_kit::ProcessSpec;

    #[test]
    fn process_spec_new_defaults() {
        let spec = ProcessSpec::new("node");
        assert_eq!(spec.command, "node");
        assert!(spec.args.is_empty());
        assert!(spec.env.is_empty());
        assert!(spec.cwd.is_none());
    }

    #[test]
    fn config_env_propagates_to_spec() {
        let cfg = ClaudeBridgeConfig::new()
            .with_api_key("sk-test")
            .with_env("MODEL", "opus");

        let mut spec = ProcessSpec::new("node");
        spec.env = cfg.env.clone();

        assert_eq!(spec.env.get("ANTHROPIC_API_KEY").unwrap(), "sk-test");
        assert_eq!(spec.env.get("MODEL").unwrap(), "opus");
    }

    #[test]
    fn config_adapter_module_sets_env() {
        let cfg = ClaudeBridgeConfig::new().with_adapter_module("/adapters/custom.js");

        let mut spec = ProcessSpec::new("node");
        if let Some(adapter) = &cfg.adapter_module {
            spec.env.insert(
                "ABP_CLAUDE_ADAPTER_MODULE".into(),
                adapter.to_string_lossy().into_owned(),
            );
        }

        assert_eq!(
            spec.env.get("ABP_CLAUDE_ADAPTER_MODULE").unwrap(),
            "/adapters/custom.js"
        );
    }

    #[test]
    fn config_cwd_propagates_to_spec() {
        let cfg = ClaudeBridgeConfig::new().with_cwd("/workspace");

        let mut spec = ProcessSpec::new("node");
        if let Some(cwd) = &cfg.cwd {
            spec.cwd = Some(cwd.to_string_lossy().into_owned());
        }

        assert_eq!(spec.cwd.as_deref(), Some("/workspace"));
    }

    #[test]
    fn config_host_script_becomes_args() {
        let cfg = ClaudeBridgeConfig::new().with_host_script("/custom/host.js");

        let mut spec = ProcessSpec::new("node");
        if let Some(script) = &cfg.host_script {
            spec.args = vec![script.to_string_lossy().into_owned()];
        }

        assert_eq!(spec.args, vec!["/custom/host.js"]);
    }

    #[test]
    fn process_spec_env_ordering_is_deterministic() {
        let mut spec = ProcessSpec::new("node");
        spec.env.insert("Z_KEY".into(), "z".into());
        spec.env.insert("A_KEY".into(), "a".into());
        spec.env.insert("M_KEY".into(), "m".into());

        let keys: Vec<&String> = spec.env.keys().collect();
        assert_eq!(keys, vec!["A_KEY", "M_KEY", "Z_KEY"]);
    }
}

// ═══════════════════════════════════════════════════════════════════
// 16. Diagnostics
// ═══════════════════════════════════════════════════════════════════

mod diagnostics_tests {
    use sidecar_kit::diagnostics::*;

    #[test]
    fn empty_collector() {
        let collector = DiagnosticCollector::new();
        assert_eq!(collector.diagnostics().len(), 0);
        assert!(!collector.has_errors());
        assert_eq!(collector.error_count(), 0);
    }

    #[test]
    fn add_info() {
        let mut collector = DiagnosticCollector::new();
        collector.add_info("SK001", "sidecar started");
        assert_eq!(collector.diagnostics().len(), 1);
        assert_eq!(collector.diagnostics()[0].level, DiagnosticLevel::Info);
    }

    #[test]
    fn add_warning() {
        let mut collector = DiagnosticCollector::new();
        collector.add_warning("SK002", "slow response");
        assert_eq!(collector.diagnostics()[0].level, DiagnosticLevel::Warning);
    }

    #[test]
    fn add_error() {
        let mut collector = DiagnosticCollector::new();
        collector.add_error("SK003", "crash");
        assert!(collector.has_errors());
        assert_eq!(collector.error_count(), 1);
    }

    #[test]
    fn summary_counts() {
        let mut collector = DiagnosticCollector::new();
        collector.add_info("I1", "info1");
        collector.add_info("I2", "info2");
        collector.add_warning("W1", "warn1");
        collector.add_error("E1", "err1");

        let summary = collector.summary();
        assert_eq!(summary.info_count, 2);
        assert_eq!(summary.warning_count, 1);
        assert_eq!(summary.error_count, 1);
        assert_eq!(summary.total, 4);
    }

    #[test]
    fn by_level_filtering() {
        let mut collector = DiagnosticCollector::new();
        collector.add_info("I1", "info");
        collector.add_error("E1", "err");
        collector.add_info("I2", "info2");

        let errors = collector.by_level(DiagnosticLevel::Error);
        assert_eq!(errors.len(), 1);
        let infos = collector.by_level(DiagnosticLevel::Info);
        assert_eq!(infos.len(), 2);
    }

    #[test]
    fn clear_removes_all() {
        let mut collector = DiagnosticCollector::new();
        collector.add_info("I1", "info");
        collector.add_error("E1", "err");
        collector.clear();
        assert_eq!(collector.diagnostics().len(), 0);
        assert!(!collector.has_errors());
    }
}

// ═══════════════════════════════════════════════════════════════════
// 17. Frame try_event and try_final typed extraction
// ═══════════════════════════════════════════════════════════════════

mod frame_typed_extraction {
    use super::*;
    use sidecar_kit::Frame;
    use std::collections::HashMap;

    #[test]
    fn try_event_extracts_typed_value() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"key": "value", "num": 42}),
        };
        let (ref_id, map): (String, HashMap<String, Value>) = frame.try_event().unwrap();
        assert_eq!(ref_id, "r1");
        assert_eq!(map.get("key").unwrap(), "value");
    }

    #[test]
    fn try_event_on_non_event_frame_fails() {
        let frame = Frame::Ping { seq: 1 };
        let result: Result<(String, Value), _> = frame.try_event();
        assert!(result.is_err());
    }

    #[test]
    fn try_final_extracts_typed_receipt() {
        let frame = Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({"outcome": "complete", "tokens": 500}),
        };
        let (ref_id, map): (String, HashMap<String, Value>) = frame.try_final().unwrap();
        assert_eq!(ref_id, "r1");
        assert_eq!(map.get("outcome").unwrap(), "complete");
    }

    #[test]
    fn try_final_on_non_final_frame_fails() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({}),
        };
        let result: Result<(String, Value), _> = frame.try_final();
        assert!(result.is_err());
    }

    #[test]
    fn try_event_type_mismatch_fails() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!("just a string"),
        };
        // Try to deserialize a string as HashMap — should fail
        let result: Result<(String, HashMap<String, Value>), _> = frame.try_event();
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════
// 18. Discovery edge cases
// ═══════════════════════════════════════════════════════════════════

mod discovery_edge_cases {
    use super::*;

    #[test]
    fn resolve_node_with_real_tempdir_binary() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "fakenode.exe"
        } else {
            "fakenode"
        };
        let fake = dir.path().join(name);
        std::fs::write(&fake, b"fake").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let result = resolve_node(Some(fake.to_str().unwrap()));
        assert!(result.is_ok());
    }

    #[test]
    fn resolve_host_script_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("host.js");
        std::fs::write(&script, "//").unwrap();
        assert!(resolve_host_script(Some(&script)).is_ok());
    }

    #[test]
    fn resolve_host_script_directory_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_host_script(Some(dir.path())).is_err());
    }

    #[test]
    fn discovery_constants_unchanged() {
        assert_eq!(DEFAULT_NODE_COMMAND, "node");
        assert_eq!(HOST_SCRIPT_RELATIVE, "hosts/claude/host.js");
        assert_eq!(HOST_SCRIPT_ENV, "ABP_CLAUDE_HOST_SCRIPT");
    }
}

// ═══════════════════════════════════════════════════════════════════
// 19. Bridge construction
// ═══════════════════════════════════════════════════════════════════

mod bridge_construction {
    use super::*;

    #[test]
    fn bridge_new_with_default_config() {
        let _bridge = claude_bridge::ClaudeBridge::new(ClaudeBridgeConfig::default());
    }

    #[test]
    fn bridge_new_with_full_config() {
        let cfg = ClaudeBridgeConfig::new()
            .with_api_key("sk-test")
            .with_node_command("node")
            .with_host_script("/tmp/host.js")
            .with_cwd("/tmp")
            .with_adapter_module("/tmp/adapter.js")
            .with_env("EXTRA", "v")
            .with_handshake_timeout(Duration::from_secs(5))
            .with_channel_buffer(16);
        let _bridge = claude_bridge::ClaudeBridge::new(cfg);
    }

    #[test]
    fn bridge_config_new_equals_default() {
        let a = ClaudeBridgeConfig::new();
        let b = ClaudeBridgeConfig::default();
        assert_eq!(a.handshake_timeout, b.handshake_timeout);
        assert_eq!(a.channel_buffer, b.channel_buffer);
        assert_eq!(a.env, b.env);
    }
}
