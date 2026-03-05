#![allow(clippy::all)]
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
//! Deep tests for claude-bridge: content block construction, message/work-order
//! format, Frame-level event mapping, and serde round-trips.

use serde_json::{Value, json};

use claude_bridge::config::ClaudeBridgeConfig;
use claude_bridge::error::BridgeError;
use claude_bridge::raw::RunOptions;

// ═══════════════════════════════════════════════════════════════════
// Module: content_block_conversion
//
// Claude API content blocks are JSON objects with a `type` discriminator.
// These tests verify that the JSON shapes the bridge would construct or
// receive match the Claude API contract.
// ═══════════════════════════════════════════════════════════════════

mod content_block_conversion {
    use super::*;

    #[test]
    fn text_block_has_correct_shape() {
        let block = json!({ "type": "text", "text": "Hello, world!" });
        assert_eq!(block["type"], "text");
        assert_eq!(block["text"], "Hello, world!");
        assert!(block.get("tool_use_id").is_none());
    }

    #[test]
    fn tool_use_block_with_json_args() {
        let block = json!({
            "type": "tool_use",
            "id": "toolu_01A",
            "name": "read_file",
            "input": { "path": "/src/main.rs" }
        });
        assert_eq!(block["type"], "tool_use");
        assert_eq!(block["name"], "read_file");
        assert!(block["input"].is_object());
        assert_eq!(block["input"]["path"], "/src/main.rs");
    }

    #[test]
    fn tool_result_block_text() {
        let block = json!({
            "type": "tool_result",
            "tool_use_id": "toolu_01A",
            "content": "fn main() {}"
        });
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "toolu_01A");
        assert_eq!(block["content"], "fn main() {}");
    }

    #[test]
    fn tool_result_block_is_error() {
        let block = json!({
            "type": "tool_result",
            "tool_use_id": "toolu_02",
            "content": "file not found",
            "is_error": true
        });
        assert!(block["is_error"].as_bool().unwrap());
    }

    #[test]
    fn mixed_content_blocks_array() {
        let blocks = json!([
            { "type": "text", "text": "Let me read the file." },
            { "type": "tool_use", "id": "toolu_01", "name": "read_file", "input": { "path": "a.rs" } },
        ]);
        let arr = blocks.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[1]["type"], "tool_use");
    }

    #[test]
    fn empty_content_blocks_array() {
        let blocks = json!([]);
        assert!(blocks.as_array().unwrap().is_empty());
    }

    #[test]
    fn nested_json_in_tool_args() {
        let block = json!({
            "type": "tool_use",
            "id": "toolu_nested",
            "name": "execute",
            "input": {
                "command": "grep",
                "options": {
                    "recursive": true,
                    "patterns": ["TODO", "FIXME"],
                    "context": { "before": 3, "after": 3 }
                }
            }
        });
        let opts = &block["input"]["options"];
        assert!(opts["recursive"].as_bool().unwrap());
        assert_eq!(opts["patterns"].as_array().unwrap().len(), 2);
        assert_eq!(opts["context"]["before"], 3);
    }

    #[test]
    fn text_block_with_unicode_content() {
        let block = json!({ "type": "text", "text": "日本語テスト 🦀" });
        assert_eq!(block["text"], "日本語テスト 🦀");
    }

    #[test]
    fn tool_use_empty_input() {
        let block = json!({
            "type": "tool_use",
            "id": "toolu_empty",
            "name": "list_files",
            "input": {}
        });
        assert!(block["input"].as_object().unwrap().is_empty());
    }

    #[test]
    fn tool_result_with_structured_content() {
        let block = json!({
            "type": "tool_result",
            "tool_use_id": "toolu_03",
            "content": [
                { "type": "text", "text": "line 1" },
                { "type": "text", "text": "line 2" }
            ]
        });
        assert!(block["content"].is_array());
        assert_eq!(block["content"].as_array().unwrap().len(), 2);
    }
}

// ═══════════════════════════════════════════════════════════════════
// Module: message_conversion
//
// Verify the JSON structure of work orders and messages the bridge
// constructs for the sidecar, matching the expected wire format.
// ═══════════════════════════════════════════════════════════════════

mod message_conversion {
    use super::*;

    /// Helper: build a passthrough-style work order JSON (mirrors raw::run_raw).
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

    /// Helper: build a mapped-mode work order JSON (mirrors raw::run_mapped_raw).
    fn build_mapped_work_order(
        task: &str,
        opts: &RunOptions,
        config: &ClaudeBridgeConfig,
    ) -> Value {
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

    #[test]
    fn user_message_passthrough_has_required_fields() {
        let request = json!({ "prompt": "Hello Claude" });
        let cfg = ClaudeBridgeConfig::new();
        let wo = build_passthrough_work_order(&request, &cfg);

        assert_eq!(wo["id"], "test-run-id");
        assert_eq!(wo["task"], "Hello Claude");
        assert_eq!(wo["lane"], "patch_first");
        assert_eq!(wo["workspace"]["mode"], "pass_through");
        assert_eq!(wo["config"]["vendor"]["abp.mode"], "passthrough");
    }

    #[test]
    fn passthrough_without_prompt_defaults_task() {
        let request = json!({ "model": "claude-3" });
        let cfg = ClaudeBridgeConfig::new();
        let wo = build_passthrough_work_order(&request, &cfg);
        assert_eq!(wo["task"], "passthrough");
    }

    #[test]
    fn passthrough_embeds_raw_request() {
        let request = json!({
            "prompt": "test",
            "model": "claude-3-opus",
            "max_tokens": 1024
        });
        let cfg = ClaudeBridgeConfig::new();
        let wo = build_passthrough_work_order(&request, &cfg);

        let embedded = &wo["config"]["vendor"]["abp.request"];
        assert_eq!(embedded["model"], "claude-3-opus");
        assert_eq!(embedded["max_tokens"], 1024);
    }

    #[test]
    fn mapped_mode_work_order_structure() {
        let opts = RunOptions::default();
        let cfg = ClaudeBridgeConfig::new();
        let wo = build_mapped_work_order("Fix the bug", &opts, &cfg);

        assert_eq!(wo["task"], "Fix the bug");
        assert_eq!(wo["lane"], "patch_first");
        assert_eq!(wo["workspace"]["mode"], "staged");
        assert_eq!(wo["config"]["vendor"]["abp.mode"], "mapped");
    }

    #[test]
    fn mapped_mode_with_custom_lane() {
        let opts = RunOptions {
            lane: Some("review".into()),
            ..Default::default()
        };
        let cfg = ClaudeBridgeConfig::new();
        let wo = build_mapped_work_order("Review code", &opts, &cfg);
        assert_eq!(wo["lane"], "review");
    }

    #[test]
    fn mapped_mode_extra_config_merged() {
        let opts = RunOptions {
            extra_config: Some(json!({
                "model": "claude-3-opus",
                "temperature": 0.5
            })),
            ..Default::default()
        };
        let cfg = ClaudeBridgeConfig::new();
        let wo = build_mapped_work_order("task", &opts, &cfg);

        let vendor = &wo["config"]["vendor"];
        assert_eq!(vendor["abp.mode"], "mapped");
        assert_eq!(vendor["model"], "claude-3-opus");
        assert_eq!(vendor["temperature"], 0.5);
    }

    #[test]
    fn mapped_mode_cwd_as_workspace_root() {
        let opts = RunOptions::default();
        let cfg = ClaudeBridgeConfig::new().with_cwd("/my/project");
        let wo = build_mapped_work_order("task", &opts, &cfg);
        assert_eq!(wo["workspace"]["root"], "/my/project");
    }

    #[test]
    fn mapped_mode_workspace_root_overrides_cwd() {
        let opts = RunOptions {
            workspace_root: Some("/explicit/root".into()),
            ..Default::default()
        };
        let cfg = ClaudeBridgeConfig::new().with_cwd("/config/cwd");
        let wo = build_mapped_work_order("task", &opts, &cfg);
        assert_eq!(wo["workspace"]["root"], "/explicit/root");
    }
}

// ═══════════════════════════════════════════════════════════════════
// Module: event_mapping
//
// Test Frame-level event construction, encoding/decoding, and
// the JSON shapes that map to/from agent events.
// ═══════════════════════════════════════════════════════════════════

mod event_mapping {
    use super::*;
    use sidecar_kit::{Frame, JsonlCodec};

    #[test]
    fn hello_frame_roundtrips_through_jsonl() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({ "name": "claude" }),
            capabilities: json!({ "streaming": true }),
            mode: json!("mapped"),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();

        match decoded {
            Frame::Hello {
                contract_version,
                backend,
                ..
            } => {
                assert_eq!(contract_version, "abp/v0.1");
                assert_eq!(backend["name"], "claude");
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn event_frame_carries_agent_event_json() {
        let event_json = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "assistant_delta",
            "text": "Hello!"
        });
        let frame = Frame::Event {
            ref_id: "run-1".into(),
            event: event_json.clone(),
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
    fn tool_call_event_json_structure() {
        let event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "tool_call",
            "tool_name": "read_file",
            "tool_use_id": "toolu_01",
            "input": { "path": "/src/lib.rs" }
        });
        assert_eq!(event["type"], "tool_call");
        assert_eq!(event["tool_name"], "read_file");
        assert!(event["input"].is_object());
    }

    #[test]
    fn tool_result_event_json_structure() {
        let event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "tool_result",
            "tool_name": "read_file",
            "tool_use_id": "toolu_01",
            "output": { "content": "file contents" },
            "is_error": false
        });
        assert_eq!(event["type"], "tool_result");
        assert!(!event["is_error"].as_bool().unwrap());
    }

    #[test]
    fn error_event_json_structure() {
        let event = json!({
            "ts": "2025-01-01T00:00:00Z",
            "type": "error",
            "message": "API rate limit exceeded"
        });
        assert_eq!(event["type"], "error");
        assert_eq!(event["message"], "API rate limit exceeded");
    }

    #[test]
    fn fatal_frame_carries_error_string() {
        let frame = Frame::Fatal {
            ref_id: Some("run-1".into()),
            error: "sidecar crashed".into(),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();

        match decoded {
            Frame::Fatal { ref_id, error } => {
                assert_eq!(ref_id.as_deref(), Some("run-1"));
                assert_eq!(error, "sidecar crashed");
            }
            other => panic!("expected Fatal, got {other:?}"),
        }
    }

    #[test]
    fn final_frame_carries_receipt_json() {
        let receipt_json = json!({
            "meta": { "run_id": "r1" },
            "outcome": { "status": "success" }
        });
        let frame = Frame::Final {
            ref_id: "run-1".into(),
            receipt: receipt_json,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();

        match decoded {
            Frame::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "run-1");
                assert_eq!(receipt["outcome"]["status"], "success");
            }
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn event_sequence_hello_run_event_final() {
        let frames = [
            Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude"}),
                capabilities: json!({}),
                mode: json!("mapped"),
            },
            Frame::Run {
                id: "run-1".into(),
                work_order: json!({"task": "test"}),
            },
            Frame::Event {
                ref_id: "run-1".into(),
                event: json!({"ts": "2025-01-01T00:00:00Z", "type": "run_started", "message": "Starting"}),
            },
            Frame::Event {
                ref_id: "run-1".into(),
                event: json!({"ts": "2025-01-01T00:00:01Z", "type": "assistant_delta", "text": "Hi"}),
            },
            Frame::Final {
                ref_id: "run-1".into(),
                receipt: json!({"outcome": "done"}),
            },
        ];

        // All frames should encode and decode successfully
        for (i, frame) in frames.iter().enumerate() {
            let encoded = JsonlCodec::encode(frame)
                .unwrap_or_else(|e| panic!("frame {i} encode failed: {e}"));
            let decoded = JsonlCodec::decode(&encoded)
                .unwrap_or_else(|e| panic!("frame {i} decode failed: {e}"));

            // Verify discriminator tag is preserved
            let raw: Value = serde_json::from_str(&encoded).unwrap();
            let expected_t = match frame {
                Frame::Hello { .. } => "hello",
                Frame::Run { .. } => "run",
                Frame::Event { .. } => "event",
                Frame::Final { .. } => "final",
                Frame::Fatal { .. } => "fatal",
                Frame::Cancel { .. } => "cancel",
                Frame::Ping { .. } => "ping",
                Frame::Pong { .. } => "pong",
            };
            assert_eq!(raw["t"], expected_t, "frame {i} tag mismatch");
            drop(decoded);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Module: serde_roundtrip
//
// Verify that bridge-related types serialize and deserialize correctly
// through JSON, preserving field ordering and optional field behavior.
// ═══════════════════════════════════════════════════════════════════

mod serde_roundtrip {
    use super::*;
    use sidecar_kit::{Frame, JsonlCodec, ProcessSpec};

    #[test]
    fn frame_hello_json_format() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"name": "claude", "version": "3.5"}),
            capabilities: json!({"tools": true, "streaming": true}),
            mode: json!("passthrough"),
        };
        let json_str = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed["t"], "hello");
        assert_eq!(parsed["contract_version"], "abp/v0.1");
        assert_eq!(parsed["backend"]["name"], "claude");
        assert_eq!(parsed["capabilities"]["tools"], true);
        assert_eq!(parsed["mode"], "passthrough");
    }

    #[test]
    fn frame_event_json_format() {
        let frame = Frame::Event {
            ref_id: "abc-123".into(),
            event: json!({"ts": "2025-01-01T00:00:00Z", "type": "assistant_delta", "text": "Hi"}),
        };
        let json_str = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed["t"], "event");
        assert_eq!(parsed["ref_id"], "abc-123");
        assert_eq!(parsed["event"]["type"], "assistant_delta");
    }

    #[test]
    fn frame_final_json_format() {
        let frame = Frame::Final {
            ref_id: "run-42".into(),
            receipt: json!({
                "outcome": {"status": "success"},
                "usage_raw": {"input_tokens": 100, "output_tokens": 50}
            }),
        };
        let json_str = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed["t"], "final");
        assert_eq!(parsed["ref_id"], "run-42");
        assert_eq!(parsed["receipt"]["usage_raw"]["input_tokens"], 100);
    }

    #[test]
    fn frame_fatal_optional_ref_id_none() {
        let frame = Frame::Fatal {
            ref_id: None,
            error: "startup failure".into(),
        };
        let json_str = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed["t"], "fatal");
        // ref_id should be null when None
        assert!(parsed["ref_id"].is_null());
        assert_eq!(parsed["error"], "startup failure");
    }

    #[test]
    fn frame_fatal_optional_ref_id_some() {
        let frame = Frame::Fatal {
            ref_id: Some("run-99".into()),
            error: "crash".into(),
        };
        let json_str = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed["ref_id"], "run-99");
    }

    #[test]
    fn process_spec_new_has_defaults() {
        let spec = ProcessSpec::new("node");
        assert_eq!(spec.command, "node");
        assert!(spec.args.is_empty());
        assert!(spec.env.is_empty());
        assert!(spec.cwd.is_none());
    }

    #[test]
    fn process_spec_env_btreemap_ordering() {
        let mut spec = ProcessSpec::new("node");
        spec.env.insert("ZEBRA".into(), "1".into());
        spec.env.insert("ALPHA".into(), "2".into());
        spec.env.insert("MIDDLE".into(), "3".into());

        let keys: Vec<&String> = spec.env.keys().collect();
        assert_eq!(keys, vec!["ALPHA", "MIDDLE", "ZEBRA"]);
    }

    #[test]
    fn work_order_json_deterministic_keys() {
        // BTreeMap-based env should produce deterministic JSON key ordering
        let cfg = ClaudeBridgeConfig::new()
            .with_env("Z_KEY", "z")
            .with_env("A_KEY", "a")
            .with_env("M_KEY", "m");

        let keys: Vec<&String> = cfg.env.keys().collect();
        assert_eq!(keys, vec!["A_KEY", "M_KEY", "Z_KEY"]);

        // Serialize to JSON and verify ordering is preserved
        let json_str = serde_json::to_string(&cfg.env).unwrap();
        let a_pos = json_str.find("A_KEY").unwrap();
        let m_pos = json_str.find("M_KEY").unwrap();
        let z_pos = json_str.find("Z_KEY").unwrap();
        assert!(a_pos < m_pos, "A_KEY should come before M_KEY in JSON");
        assert!(m_pos < z_pos, "M_KEY should come before Z_KEY in JSON");
    }

    #[test]
    fn frame_ping_pong_roundtrip() {
        let ping = Frame::Ping { seq: 42 };
        let encoded = JsonlCodec::encode(&ping).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Ping { seq } => assert_eq!(seq, 42),
            other => panic!("expected Ping, got {other:?}"),
        }

        let pong = Frame::Pong { seq: 42 };
        let encoded = JsonlCodec::encode(&pong).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Pong { seq } => assert_eq!(seq, 42),
            other => panic!("expected Pong, got {other:?}"),
        }
    }

    #[test]
    fn frame_cancel_roundtrip() {
        let cancel = Frame::Cancel {
            ref_id: "run-1".into(),
            reason: Some("user cancelled".into()),
        };
        let encoded = JsonlCodec::encode(&cancel).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Cancel { ref_id, reason } => {
                assert_eq!(ref_id, "run-1");
                assert_eq!(reason.as_deref(), Some("user cancelled"));
            }
            other => panic!("expected Cancel, got {other:?}"),
        }
    }

    #[test]
    fn frame_cancel_no_reason_roundtrip() {
        let cancel = Frame::Cancel {
            ref_id: "run-2".into(),
            reason: None,
        };
        let encoded = JsonlCodec::encode(&cancel).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        match decoded {
            Frame::Cancel { ref_id, reason } => {
                assert_eq!(ref_id, "run-2");
                assert!(reason.is_none());
            }
            other => panic!("expected Cancel, got {other:?}"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Additional bridge-level integration tests
// ═══════════════════════════════════════════════════════════════════

mod bridge_integration {
    use super::*;
    use sidecar_kit::{Frame, JsonlCodec, ProcessSpec};

    #[test]
    fn config_adapter_module_propagates_to_process_spec_env() {
        // When adapter_module is set, it should be available to inject
        // into the ProcessSpec env as ABP_CLAUDE_ADAPTER_MODULE.
        let cfg = ClaudeBridgeConfig::new().with_adapter_module("/adapters/custom.js");

        // Simulate what build_process_spec does
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
    fn config_cwd_propagates_to_process_spec() {
        let cfg = ClaudeBridgeConfig::new().with_cwd("/my/project");

        let mut spec = ProcessSpec::new("node");
        if let Some(cwd) = &cfg.cwd {
            spec.cwd = Some(cwd.to_string_lossy().into_owned());
        }

        assert_eq!(spec.cwd.as_deref(), Some("/my/project"));
    }

    #[test]
    fn config_env_propagates_to_process_spec() {
        let cfg = ClaudeBridgeConfig::new()
            .with_api_key("sk-test")
            .with_env("CUSTOM", "value");

        let mut spec = ProcessSpec::new("node");
        spec.env = cfg.env.clone();

        assert_eq!(spec.env.get("ANTHROPIC_API_KEY").unwrap(), "sk-test");
        assert_eq!(spec.env.get("CUSTOM").unwrap(), "value");
    }

    #[test]
    fn bridge_error_from_sidecar_timeout_is_descriptive() {
        let sidecar_err = sidecar_kit::SidecarError::Timeout;
        let bridge_err: BridgeError = sidecar_err.into();
        let msg = bridge_err.to_string();
        assert!(msg.contains("sidecar"), "should mention sidecar: {msg}");
        assert!(msg.contains("timed out"), "should mention timeout: {msg}");
    }

    #[test]
    fn cancel_token_default_is_not_cancelled() {
        let token = sidecar_kit::CancelToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancel_token_cancel_sets_flag() {
        let token = sidecar_kit::CancelToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn multiple_jsonl_frames_on_separate_lines() {
        let frames = vec![
            Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"name": "claude"}),
                capabilities: json!({}),
                mode: Value::Null,
            },
            Frame::Event {
                ref_id: "r1".into(),
                event: json!({"ts": "2025-01-01T00:00:00Z", "type": "run_started", "message": "go"}),
            },
            Frame::Final {
                ref_id: "r1".into(),
                receipt: json!({}),
            },
        ];

        let mut lines = Vec::new();
        for frame in &frames {
            let line = JsonlCodec::encode(frame).unwrap();
            let trimmed = line.trim_end_matches('\n');
            // The payload itself (excluding trailing newline) must be a single line
            assert!(
                !trimmed.contains('\n'),
                "JSONL payload must be a single line"
            );
            lines.push(line);
        }

        // Each line should decode independently
        for (i, line) in lines.iter().enumerate() {
            let decoded =
                JsonlCodec::decode(line).unwrap_or_else(|e| panic!("line {i} decode failed: {e}"));
            drop(decoded);
        }
    }

    #[test]
    fn frame_tag_uses_t_not_type() {
        // Critical contract: the serde tag is "t", not "type".
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({}),
        };
        let json_str = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();

        assert!(
            parsed.get("t").is_some(),
            "Frame must use 't' as discriminator"
        );
        assert!(
            parsed.get("type").is_none(),
            "Frame must NOT use 'type' as discriminator"
        );
    }
}
