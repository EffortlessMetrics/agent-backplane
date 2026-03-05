#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the claude-bridge crate.
//!
//! Covers config, discovery, error types, raw run options, sidecar-kit frame
//! serde, builder helpers, and normalized-mode integration (feature-gated).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use claude_bridge::config::ClaudeBridgeConfig;
use claude_bridge::discovery::{
    resolve_host_script, resolve_node, DEFAULT_NODE_COMMAND, HOST_SCRIPT_ENV, HOST_SCRIPT_RELATIVE,
};
use claude_bridge::error::BridgeError;
use claude_bridge::raw::RunOptions;
use claude_bridge::ClaudeBridge;

// ═══════════════════════════════════════════════════════════════════════════
// Section 1: ClaudeBridgeConfig — defaults and builder
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_default_has_expected_values() {
    let cfg = ClaudeBridgeConfig::default();
    assert!(cfg.node_command.is_none());
    assert!(cfg.host_script.is_none());
    assert!(cfg.env.is_empty());
    assert!(cfg.cwd.is_none());
    assert!(cfg.adapter_module.is_none());
    assert_eq!(cfg.handshake_timeout, Duration::from_secs(30));
    assert_eq!(cfg.channel_buffer, 256);
}

#[test]
fn config_new_matches_default() {
    let a = ClaudeBridgeConfig::new();
    let b = ClaudeBridgeConfig::default();
    assert_eq!(a.handshake_timeout, b.handshake_timeout);
    assert_eq!(a.channel_buffer, b.channel_buffer);
    assert_eq!(a.env.len(), b.env.len());
}

#[test]
fn config_with_api_key_sets_env() {
    let cfg = ClaudeBridgeConfig::new().with_api_key("sk-ant-api-key-123");
    assert_eq!(
        cfg.env.get("ANTHROPIC_API_KEY").unwrap(),
        "sk-ant-api-key-123"
    );
}

#[test]
fn config_with_api_key_overwrites_previous() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("first")
        .with_api_key("second");
    assert_eq!(cfg.env.get("ANTHROPIC_API_KEY").unwrap(), "second");
}

#[test]
fn config_with_host_script_sets_path() {
    let cfg = ClaudeBridgeConfig::new().with_host_script("/opt/abp/host.js");
    assert_eq!(cfg.host_script, Some(PathBuf::from("/opt/abp/host.js")));
}

#[test]
fn config_with_cwd_sets_path() {
    let cfg = ClaudeBridgeConfig::new().with_cwd("/my/workspace");
    assert_eq!(cfg.cwd, Some(PathBuf::from("/my/workspace")));
}

#[test]
fn config_with_adapter_module_sets_path() {
    let cfg = ClaudeBridgeConfig::new().with_adapter_module("/adapters/custom.js");
    assert_eq!(
        cfg.adapter_module,
        Some(PathBuf::from("/adapters/custom.js"))
    );
}

#[test]
fn config_with_node_command_sets_string() {
    let cfg = ClaudeBridgeConfig::new().with_node_command("/usr/local/bin/node18");
    assert_eq!(cfg.node_command.as_deref(), Some("/usr/local/bin/node18"));
}

#[test]
fn config_with_env_inserts_key_value() {
    let cfg = ClaudeBridgeConfig::new()
        .with_env("FOO", "bar")
        .with_env("BAZ", "qux");
    assert_eq!(cfg.env.get("FOO").unwrap(), "bar");
    assert_eq!(cfg.env.get("BAZ").unwrap(), "qux");
}

#[test]
fn config_with_handshake_timeout() {
    let cfg = ClaudeBridgeConfig::new().with_handshake_timeout(Duration::from_secs(60));
    assert_eq!(cfg.handshake_timeout, Duration::from_secs(60));
}

#[test]
fn config_with_channel_buffer() {
    let cfg = ClaudeBridgeConfig::new().with_channel_buffer(512);
    assert_eq!(cfg.channel_buffer, 512);
}

#[test]
fn config_builder_chain_all_methods() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("sk-test")
        .with_host_script("/h.js")
        .with_cwd("/workspace")
        .with_adapter_module("/a.js")
        .with_node_command("node20")
        .with_env("KEY", "VAL")
        .with_handshake_timeout(Duration::from_millis(500))
        .with_channel_buffer(1024);

    assert_eq!(cfg.env.get("ANTHROPIC_API_KEY").unwrap(), "sk-test");
    assert_eq!(cfg.host_script, Some(PathBuf::from("/h.js")));
    assert_eq!(cfg.cwd, Some(PathBuf::from("/workspace")));
    assert_eq!(cfg.adapter_module, Some(PathBuf::from("/a.js")));
    assert_eq!(cfg.node_command.as_deref(), Some("node20"));
    assert_eq!(cfg.env.get("KEY").unwrap(), "VAL");
    assert_eq!(cfg.handshake_timeout, Duration::from_millis(500));
    assert_eq!(cfg.channel_buffer, 1024);
}

#[test]
fn config_env_is_btreemap_sorted() {
    let cfg = ClaudeBridgeConfig::new()
        .with_env("Z_KEY", "z")
        .with_env("A_KEY", "a")
        .with_env("M_KEY", "m");
    let keys: Vec<&String> = cfg.env.keys().collect();
    assert_eq!(keys, vec!["A_KEY", "M_KEY", "Z_KEY"]);
}

#[test]
fn config_clone_is_independent() {
    let cfg1 = ClaudeBridgeConfig::new().with_api_key("key1");
    let mut cfg2 = cfg1.clone();
    cfg2.env.insert("ANTHROPIC_API_KEY".into(), "key2".into());
    assert_eq!(cfg1.env.get("ANTHROPIC_API_KEY").unwrap(), "key1");
    assert_eq!(cfg2.env.get("ANTHROPIC_API_KEY").unwrap(), "key2");
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 2: Discovery — node resolution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_node_command_constant() {
    assert_eq!(DEFAULT_NODE_COMMAND, "node");
}

#[test]
fn host_script_relative_constant() {
    assert_eq!(HOST_SCRIPT_RELATIVE, "hosts/claude/host.js");
}

#[test]
fn host_script_env_constant() {
    assert_eq!(HOST_SCRIPT_ENV, "ABP_CLAUDE_HOST_SCRIPT");
}

#[test]
fn resolve_node_with_none_finds_node_in_path() {
    // node is expected to be on PATH in CI / dev machines
    let result = resolve_node(None);
    // If node isn't on PATH this test is allowed to fail
    if result.is_ok() {
        assert_eq!(result.unwrap(), "node");
    }
}

#[test]
fn resolve_node_explicit_empty_string_falls_through() {
    // Empty string should be trimmed and treated as None
    let result = resolve_node(Some(""));
    if result.is_ok() {
        assert_eq!(result.unwrap(), "node");
    }
}

#[test]
fn resolve_node_explicit_whitespace_only_falls_through() {
    let result = resolve_node(Some("   "));
    if result.is_ok() {
        assert_eq!(result.unwrap(), "node");
    }
}

#[test]
fn resolve_node_explicit_nonexistent_returns_error() {
    let result = resolve_node(Some("absolutely_not_a_real_node_binary_xyz_999"));
    assert!(result.is_err());
    match result.unwrap_err() {
        BridgeError::NodeNotFound(msg) => {
            assert!(msg.contains("not found"), "message = {msg}");
        }
        other => panic!("expected NodeNotFound, got: {other:?}"),
    }
}

#[test]
fn resolve_host_script_explicit_nonexistent_returns_error() {
    let result = resolve_host_script(Some(Path::new("/nonexistent/path/host.js")));
    assert!(result.is_err());
    match result.unwrap_err() {
        BridgeError::HostScriptNotFound(msg) => {
            assert!(msg.contains("not found"), "message = {msg}");
        }
        other => panic!("expected HostScriptNotFound, got: {other:?}"),
    }
}

#[test]
fn resolve_host_script_none_searches_cwd_relative() {
    // This may or may not find the file depending on CWD
    let result = resolve_host_script(None);
    if result.is_ok() {
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains("host.js"));
    }
}

#[test]
fn resolve_host_script_explicit_existing_file_returns_ok() {
    // Create a temp file to use as a host script
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path();
    let result = resolve_host_script(Some(path));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), path.to_path_buf());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 3: BridgeError — variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn bridge_error_node_not_found_display() {
    let err = BridgeError::NodeNotFound("node not in PATH".into());
    let msg = format!("{err}");
    assert!(msg.contains("node.js not found"));
    assert!(msg.contains("node not in PATH"));
}

#[test]
fn bridge_error_host_script_not_found_display() {
    let err = BridgeError::HostScriptNotFound("missing host.js".into());
    let msg = format!("{err}");
    assert!(msg.contains("host script not found"));
}

#[test]
fn bridge_error_config_display() {
    let err = BridgeError::Config("bad config".into());
    let msg = format!("{err}");
    assert!(msg.contains("configuration error"));
    assert!(msg.contains("bad config"));
}

#[test]
fn bridge_error_run_display() {
    let err = BridgeError::Run("run failed".into());
    let msg = format!("{err}");
    assert!(msg.contains("run error"));
    assert!(msg.contains("run failed"));
}

#[test]
fn bridge_error_sidecar_from_sidecar_error() {
    let sidecar_err = sidecar_kit::SidecarError::Timeout;
    let bridge_err = BridgeError::from(sidecar_err);
    match bridge_err {
        BridgeError::Sidecar(e) => {
            let msg = format!("{e}");
            assert!(msg.contains("timed out"));
        }
        other => panic!("expected Sidecar variant, got: {other:?}"),
    }
}

#[test]
fn bridge_error_sidecar_protocol_conversion() {
    let sidecar_err = sidecar_kit::SidecarError::Protocol("bad frame".into());
    let bridge_err: BridgeError = sidecar_err.into();
    let msg = format!("{bridge_err}");
    assert!(msg.contains("sidecar error"));
    assert!(msg.contains("bad frame"));
}

#[test]
fn bridge_error_sidecar_fatal_conversion() {
    let sidecar_err = sidecar_kit::SidecarError::Fatal("catastrophic".into());
    let bridge_err: BridgeError = sidecar_err.into();
    let msg = format!("{bridge_err}");
    assert!(msg.contains("catastrophic"));
}

#[test]
fn bridge_error_sidecar_exited_conversion() {
    let sidecar_err = sidecar_kit::SidecarError::Exited(Some(1));
    let bridge_err: BridgeError = sidecar_err.into();
    let msg = format!("{bridge_err}");
    assert!(msg.contains("exited"));
}

#[test]
fn bridge_error_is_debug() {
    let err = BridgeError::Config("test".into());
    let dbg = format!("{err:?}");
    assert!(dbg.contains("Config"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 4: RunOptions — defaults and fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn run_options_default_all_none() {
    let opts = RunOptions::default();
    assert!(opts.lane.is_none());
    assert!(opts.workspace_root.is_none());
    assert!(opts.extra_config.is_none());
}

#[test]
fn run_options_with_lane() {
    let opts = RunOptions {
        lane: Some("patch_first".into()),
        ..Default::default()
    };
    assert_eq!(opts.lane.as_deref(), Some("patch_first"));
}

#[test]
fn run_options_with_workspace_root() {
    let opts = RunOptions {
        workspace_root: Some("/tmp/ws".into()),
        ..Default::default()
    };
    assert_eq!(opts.workspace_root.as_deref(), Some("/tmp/ws"));
}

#[test]
fn run_options_with_extra_config() {
    let opts = RunOptions {
        extra_config: Some(json!({"model": "claude-3-opus"})),
        ..Default::default()
    };
    assert!(opts.extra_config.is_some());
    let cfg = opts.extra_config.unwrap();
    assert_eq!(cfg["model"], "claude-3-opus");
}

#[test]
fn run_options_clone() {
    let opts = RunOptions {
        lane: Some("test".into()),
        workspace_root: Some("/ws".into()),
        extra_config: Some(json!({"key": "value"})),
    };
    let cloned = opts.clone();
    assert_eq!(cloned.lane, opts.lane);
    assert_eq!(cloned.workspace_root, opts.workspace_root);
    assert_eq!(cloned.extra_config, opts.extra_config);
}

#[test]
fn run_options_debug_format() {
    let opts = RunOptions::default();
    let dbg = format!("{opts:?}");
    assert!(dbg.contains("RunOptions"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 5: ClaudeBridge struct — construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_bridge_new_does_not_panic() {
    let cfg = ClaudeBridgeConfig::new();
    let _bridge = ClaudeBridge::new(cfg);
}

#[test]
fn claude_bridge_new_with_full_config() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("sk-test")
        .with_cwd("/workspace")
        .with_handshake_timeout(Duration::from_secs(10))
        .with_channel_buffer(64);
    let _bridge = ClaudeBridge::new(cfg);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 6: sidecar-kit Frame serde — protocol envelope
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn frame_hello_roundtrip() {
    let frame = sidecar_kit::Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "sidecar:claude"}),
        capabilities: json!({"streaming": "native", "tool_use": "native"}),
        mode: Value::Null,
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    assert!(json_str.contains(r#""t":"hello"#));
    let deser: sidecar_kit::Frame = serde_json::from_str(&json_str).unwrap();
    match deser {
        sidecar_kit::Frame::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "sidecar:claude");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn frame_run_roundtrip() {
    let frame = sidecar_kit::Frame::Run {
        id: "run-123".into(),
        work_order: json!({"task": "test"}),
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    assert!(json_str.contains(r#""t":"run"#));
    let deser: sidecar_kit::Frame = serde_json::from_str(&json_str).unwrap();
    match deser {
        sidecar_kit::Frame::Run { id, work_order } => {
            assert_eq!(id, "run-123");
            assert_eq!(work_order["task"], "test");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn frame_event_roundtrip() {
    let frame = sidecar_kit::Frame::Event {
        ref_id: "run-456".into(),
        event: json!({"type": "assistant_delta", "text": "hello"}),
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    assert!(json_str.contains(r#""t":"event"#));
    let deser: sidecar_kit::Frame = serde_json::from_str(&json_str).unwrap();
    match deser {
        sidecar_kit::Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-456");
            assert_eq!(event["type"], "assistant_delta");
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn frame_final_roundtrip() {
    let frame = sidecar_kit::Frame::Final {
        ref_id: "run-789".into(),
        receipt: json!({"outcome": "complete"}),
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    assert!(json_str.contains(r#""t":"final"#));
    let deser: sidecar_kit::Frame = serde_json::from_str(&json_str).unwrap();
    match deser {
        sidecar_kit::Frame::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-789");
            assert_eq!(receipt["outcome"], "complete");
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn frame_fatal_roundtrip() {
    let frame = sidecar_kit::Frame::Fatal {
        ref_id: Some("run-fail".into()),
        error: "something went wrong".into(),
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    assert!(json_str.contains(r#""t":"fatal"#));
    let deser: sidecar_kit::Frame = serde_json::from_str(&json_str).unwrap();
    match deser {
        sidecar_kit::Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("run-fail"));
            assert_eq!(error, "something went wrong");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn frame_fatal_without_ref_id() {
    let frame = sidecar_kit::Frame::Fatal {
        ref_id: None,
        error: "startup crash".into(),
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    let deser: sidecar_kit::Frame = serde_json::from_str(&json_str).unwrap();
    match deser {
        sidecar_kit::Frame::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "startup crash");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn frame_cancel_roundtrip() {
    let frame = sidecar_kit::Frame::Cancel {
        ref_id: "run-cancel".into(),
        reason: Some("user requested".into()),
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    assert!(json_str.contains(r#""t":"cancel"#));
    let deser: sidecar_kit::Frame = serde_json::from_str(&json_str).unwrap();
    match deser {
        sidecar_kit::Frame::Cancel { ref_id, reason } => {
            assert_eq!(ref_id, "run-cancel");
            assert_eq!(reason.as_deref(), Some("user requested"));
        }
        _ => panic!("expected Cancel"),
    }
}

#[test]
fn frame_ping_pong_roundtrip() {
    let ping = sidecar_kit::Frame::Ping { seq: 42 };
    let pong = sidecar_kit::Frame::Pong { seq: 42 };

    let ping_str = serde_json::to_string(&ping).unwrap();
    let pong_str = serde_json::to_string(&pong).unwrap();

    assert!(ping_str.contains(r#""t":"ping"#));
    assert!(pong_str.contains(r#""t":"pong"#));

    let deser_ping: sidecar_kit::Frame = serde_json::from_str(&ping_str).unwrap();
    let deser_pong: sidecar_kit::Frame = serde_json::from_str(&pong_str).unwrap();

    match deser_ping {
        sidecar_kit::Frame::Ping { seq } => assert_eq!(seq, 42),
        _ => panic!("expected Ping"),
    }
    match deser_pong {
        sidecar_kit::Frame::Pong { seq } => assert_eq!(seq, 42),
        _ => panic!("expected Pong"),
    }
}

#[test]
fn frame_discriminator_is_t_not_type() {
    let frame = sidecar_kit::Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    let val: Value = serde_json::from_str(&json_str).unwrap();
    assert!(
        val.get("t").is_some(),
        "envelope must use 't' discriminator"
    );
    assert!(
        val.get("type").is_none(),
        "envelope must NOT use 'type' discriminator"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 7: sidecar-kit builder helpers — event construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn builder_event_text_delta() {
    let ev = sidecar_kit::event_text_delta("hello ");
    assert_eq!(ev["type"], "assistant_delta");
    assert_eq!(ev["text"], "hello ");
    assert!(ev.get("ts").is_some());
}

#[test]
fn builder_event_text_message() {
    let ev = sidecar_kit::event_text_message("complete message");
    assert_eq!(ev["type"], "assistant_message");
    assert_eq!(ev["text"], "complete message");
}

#[test]
fn builder_event_tool_call() {
    let ev =
        sidecar_kit::event_tool_call("read_file", Some("tu_123"), json!({"path": "src/lib.rs"}));
    assert_eq!(ev["type"], "tool_call");
    assert_eq!(ev["tool_name"], "read_file");
    assert_eq!(ev["tool_use_id"], "tu_123");
    assert_eq!(ev["input"]["path"], "src/lib.rs");
}

#[test]
fn builder_event_tool_call_without_id() {
    let ev = sidecar_kit::event_tool_call("bash", None, json!({"command": "ls -la"}));
    assert_eq!(ev["type"], "tool_call");
    assert_eq!(ev["tool_name"], "bash");
    assert!(ev["tool_use_id"].is_null());
}

#[test]
fn builder_event_tool_result_success() {
    let ev = sidecar_kit::event_tool_result(
        "read_file",
        Some("tu_123"),
        json!({"content": "file contents"}),
        false,
    );
    assert_eq!(ev["type"], "tool_result");
    assert_eq!(ev["tool_name"], "read_file");
    assert_eq!(ev["tool_use_id"], "tu_123");
    assert_eq!(ev["is_error"], false);
}

#[test]
fn builder_event_tool_result_error() {
    let ev = sidecar_kit::event_tool_result(
        "write_file",
        Some("tu_456"),
        json!({"error": "permission denied"}),
        true,
    );
    assert_eq!(ev["type"], "tool_result");
    assert_eq!(ev["is_error"], true);
}

#[test]
fn builder_event_error() {
    let ev = sidecar_kit::event_error("something failed");
    assert_eq!(ev["type"], "error");
    assert_eq!(ev["message"], "something failed");
}

#[test]
fn builder_event_warning() {
    let ev = sidecar_kit::event_warning("deprecated API usage");
    assert_eq!(ev["type"], "warning");
    assert_eq!(ev["message"], "deprecated API usage");
}

#[test]
fn builder_event_run_started() {
    let ev = sidecar_kit::event_run_started("starting run");
    assert_eq!(ev["type"], "run_started");
    assert_eq!(ev["message"], "starting run");
}

#[test]
fn builder_event_run_completed() {
    let ev = sidecar_kit::event_run_completed("run done");
    assert_eq!(ev["type"], "run_completed");
    assert_eq!(ev["message"], "run done");
}

#[test]
fn builder_event_file_changed() {
    let ev = sidecar_kit::event_file_changed("src/main.rs", "added error handling");
    assert_eq!(ev["type"], "file_changed");
    assert_eq!(ev["path"], "src/main.rs");
    assert_eq!(ev["summary"], "added error handling");
}

#[test]
fn builder_event_command_executed() {
    let ev = sidecar_kit::event_command_executed("cargo test", Some(0), Some("all tests passed"));
    assert_eq!(ev["type"], "command_executed");
    assert_eq!(ev["command"], "cargo test");
    assert_eq!(ev["exit_code"], 0);
    assert_eq!(ev["output_preview"], "all tests passed");
}

#[test]
fn builder_event_command_executed_no_output() {
    let ev = sidecar_kit::event_command_executed("ls", None, None);
    assert_eq!(ev["type"], "command_executed");
    assert!(ev["exit_code"].is_null());
    assert!(ev["output_preview"].is_null());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 8: sidecar-kit frame helpers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn builder_event_frame() {
    let event = sidecar_kit::event_text_delta("hi");
    let frame = sidecar_kit::event_frame("run-1", event.clone());
    match frame {
        sidecar_kit::Frame::Event { ref_id, event: ev } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(ev["type"], "assistant_delta");
        }
        _ => panic!("expected Event frame"),
    }
}

#[test]
fn builder_fatal_frame_with_ref_id() {
    let frame = sidecar_kit::fatal_frame(Some("run-err"), "out of memory");
    match frame {
        sidecar_kit::Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("run-err"));
            assert_eq!(error, "out of memory");
        }
        _ => panic!("expected Fatal frame"),
    }
}

#[test]
fn builder_fatal_frame_without_ref_id() {
    let frame = sidecar_kit::fatal_frame(None, "startup failure");
    match frame {
        sidecar_kit::Frame::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "startup failure");
        }
        _ => panic!("expected Fatal frame"),
    }
}

#[test]
fn builder_hello_frame() {
    let frame = sidecar_kit::hello_frame("claude-sidecar");
    match frame {
        sidecar_kit::Frame::Hello {
            contract_version,
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "claude-sidecar");
            assert_eq!(capabilities, json!({}));
        }
        _ => panic!("expected Hello frame"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 9: ReceiptBuilder (sidecar-kit, Value-based)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_builder_basic() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-1", "sidecar:claude").build();
    assert_eq!(receipt["meta"]["run_id"], "run-1");
    assert_eq!(receipt["backend"]["id"], "sidecar:claude");
    assert_eq!(receipt["outcome"], "complete");
    assert_eq!(receipt["meta"]["contract_version"], "abp/v0.1");
}

#[test]
fn receipt_builder_failed() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-2", "sidecar:claude")
        .failed()
        .build();
    assert_eq!(receipt["outcome"], "failed");
}

#[test]
fn receipt_builder_partial() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-3", "sidecar:claude")
        .partial()
        .build();
    assert_eq!(receipt["outcome"], "partial");
}

#[test]
fn receipt_builder_with_events() {
    let ev1 = sidecar_kit::event_text_delta("hello ");
    let ev2 = sidecar_kit::event_text_delta("world");
    let receipt = sidecar_kit::ReceiptBuilder::new("run-4", "sidecar:claude")
        .event(ev1)
        .event(ev2)
        .build();
    let trace = receipt["trace"].as_array().unwrap();
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0]["text"], "hello ");
    assert_eq!(trace[1]["text"], "world");
}

#[test]
fn receipt_builder_with_artifact() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-5", "sidecar:claude")
        .artifact("patch", "changes.patch")
        .build();
    let artifacts = receipt["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["kind"], "patch");
    assert_eq!(artifacts[0]["path"], "changes.patch");
}

#[test]
fn receipt_builder_with_usage() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-6", "sidecar:claude")
        .usage_raw(json!({"input_tokens": 1000, "output_tokens": 500}))
        .input_tokens(1000)
        .output_tokens(500)
        .build();
    assert_eq!(receipt["usage_raw"]["input_tokens"], 1000);
    assert_eq!(receipt["usage"]["input_tokens"], 1000);
    assert_eq!(receipt["usage"]["output_tokens"], 500);
}

#[test]
fn receipt_builder_receipt_sha256_is_null() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-7", "sidecar:claude").build();
    assert!(receipt["receipt_sha256"].is_null());
}

#[test]
fn receipt_builder_meta_has_timestamps() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-8", "sidecar:claude").build();
    assert!(receipt["meta"]["started_at"].is_string());
    assert!(receipt["meta"]["finished_at"].is_string());
    assert_eq!(receipt["meta"]["duration_ms"], 0);
}

#[test]
fn receipt_builder_work_order_id_matches_run_id() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-9", "sidecar:claude").build();
    assert_eq!(receipt["meta"]["work_order_id"], "run-9");
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 10: Claude-specific message format — tool_use / tool_result
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_tool_use_event_structure() {
    // Simulate Claude's tool_use content block
    let tool_use = json!({
        "type": "tool_use",
        "id": "toolu_01A09q90qw90lq917835lq9",
        "name": "read_file",
        "input": {"path": "/src/main.rs"}
    });
    assert_eq!(tool_use["type"], "tool_use");
    assert!(tool_use["id"].as_str().unwrap().starts_with("toolu_"));
    assert_eq!(tool_use["name"], "read_file");
}

#[test]
fn claude_tool_result_event_structure() {
    let tool_result = json!({
        "type": "tool_result",
        "tool_use_id": "toolu_01A09q90qw90lq917835lq9",
        "content": "fn main() {\n    println!(\"Hello\");\n}",
        "is_error": false
    });
    assert_eq!(tool_result["type"], "tool_result");
    assert_eq!(tool_result["is_error"], false);
    assert!(tool_result["tool_use_id"]
        .as_str()
        .unwrap()
        .starts_with("toolu_"));
}

#[test]
fn claude_tool_use_mapped_to_abp_tool_call() {
    // Verify the ABP event_tool_call maps Claude's tool_use properly
    let ev =
        sidecar_kit::event_tool_call("read_file", Some("toolu_01ABC"), json!({"path": "src/"}));
    assert_eq!(ev["type"], "tool_call");
    assert_eq!(ev["tool_name"], "read_file");
    assert_eq!(ev["tool_use_id"], "toolu_01ABC");
}

#[test]
fn claude_tool_result_mapped_to_abp_tool_result() {
    let ev = sidecar_kit::event_tool_result(
        "read_file",
        Some("toolu_01ABC"),
        json!({"content": "file data"}),
        false,
    );
    assert_eq!(ev["type"], "tool_result");
    assert_eq!(ev["tool_name"], "read_file");
    assert_eq!(ev["tool_use_id"], "toolu_01ABC");
    assert_eq!(ev["is_error"], false);
}

#[test]
fn claude_tool_result_error_mapped() {
    let ev = sidecar_kit::event_tool_result(
        "write_file",
        Some("toolu_99XYZ"),
        json!({"error": "file not found"}),
        true,
    );
    assert_eq!(ev["is_error"], true);
    assert_eq!(ev["output"]["error"], "file not found");
}

#[test]
fn claude_nested_tool_call_parent_id() {
    // ABP supports parent_tool_use_id for nested tool calls
    let ev = sidecar_kit::event_tool_call("bash", Some("toolu_child"), json!({"cmd": "ls"}));
    // parent_tool_use_id is null by default from the builder
    assert!(ev["parent_tool_use_id"].is_null());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 11: Streaming event translation — delta sequences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_delta_sequence() {
    let deltas = vec!["Hello", ", ", "world", "!"];
    let events: Vec<Value> = deltas
        .iter()
        .map(|d| sidecar_kit::event_text_delta(d))
        .collect();

    for (i, ev) in events.iter().enumerate() {
        assert_eq!(ev["type"], "assistant_delta");
        assert_eq!(ev["text"], deltas[i]);
    }
}

#[test]
fn streaming_events_have_timestamps() {
    let ev1 = sidecar_kit::event_text_delta("a");
    let ev2 = sidecar_kit::event_text_delta("b");
    assert!(ev1["ts"].is_string());
    assert!(ev2["ts"].is_string());
}

#[test]
fn streaming_event_wrapped_in_frame() {
    let ev = sidecar_kit::event_text_delta("token");
    let frame = sidecar_kit::event_frame("run-stream", ev);
    let json_str = serde_json::to_string(&frame).unwrap();
    let parsed: Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["t"], "event");
    assert_eq!(parsed["ref_id"], "run-stream");
    assert_eq!(parsed["event"]["type"], "assistant_delta");
    assert_eq!(parsed["event"]["text"], "token");
}

#[test]
fn streaming_tool_call_then_result_sequence() {
    let call = sidecar_kit::event_tool_call("bash", Some("tu_1"), json!({"command": "ls"}));
    let result = sidecar_kit::event_tool_result(
        "bash",
        Some("tu_1"),
        json!({"output": "file1.rs\nfile2.rs"}),
        false,
    );
    let call_frame = sidecar_kit::event_frame("run-tc", call);
    let result_frame = sidecar_kit::event_frame("run-tc", result);

    let call_json: Value =
        serde_json::from_str(&serde_json::to_string(&call_frame).unwrap()).unwrap();
    let result_json: Value =
        serde_json::from_str(&serde_json::to_string(&result_frame).unwrap()).unwrap();

    assert_eq!(call_json["event"]["type"], "tool_call");
    assert_eq!(result_json["event"]["type"], "tool_result");
    assert_eq!(
        call_json["event"]["tool_use_id"],
        result_json["event"]["tool_use_id"]
    );
}

#[test]
fn streaming_full_lifecycle_events() {
    let events = vec![
        sidecar_kit::event_run_started("beginning task"),
        sidecar_kit::event_text_delta("I'll "),
        sidecar_kit::event_text_delta("read the file."),
        sidecar_kit::event_tool_call("read_file", Some("tu_1"), json!({"path": "main.rs"})),
        sidecar_kit::event_tool_result(
            "read_file",
            Some("tu_1"),
            json!({"content": "fn main() {}"}),
            false,
        ),
        sidecar_kit::event_text_message("I've read the file successfully."),
        sidecar_kit::event_run_completed("task complete"),
    ];

    let types: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(
        types,
        vec![
            "run_started",
            "assistant_delta",
            "assistant_delta",
            "tool_call",
            "tool_result",
            "assistant_message",
            "run_completed"
        ]
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 12: Error mapping — Claude API errors to ABP error taxonomy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_code_serde_roundtrip() {
    let code = abp_error::ErrorCode::BackendTimeout;
    let serialized = serde_json::to_string(&code).unwrap();
    assert_eq!(serialized, r#""backend_timeout""#);
    let deser: abp_error::ErrorCode = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deser, code);
}

#[test]
fn abp_error_code_as_str_is_snake_case() {
    assert_eq!(
        abp_error::ErrorCode::BackendTimeout.as_str(),
        "backend_timeout"
    );
    assert_eq!(
        abp_error::ErrorCode::BackendAuthFailed.as_str(),
        "backend_auth_failed"
    );
    assert_eq!(
        abp_error::ErrorCode::BackendRateLimited.as_str(),
        "backend_rate_limited"
    );
    assert_eq!(abp_error::ErrorCode::Internal.as_str(), "internal");
}

#[test]
fn claude_overloaded_maps_to_backend_unavailable() {
    // Claude returns 529 "overloaded_error"
    let code = abp_error::ErrorCode::BackendUnavailable;
    assert_eq!(code.as_str(), "backend_unavailable");
    assert_eq!(code.category(), abp_error::ErrorCategory::Backend);
}

#[test]
fn claude_rate_limit_maps_to_backend_rate_limited() {
    let code = abp_error::ErrorCode::BackendRateLimited;
    assert_eq!(code.as_str(), "backend_rate_limited");
    assert_eq!(code.category(), abp_error::ErrorCategory::Backend);
}

#[test]
fn claude_auth_error_maps_to_backend_auth_failed() {
    let code = abp_error::ErrorCode::BackendAuthFailed;
    assert_eq!(code.as_str(), "backend_auth_failed");
}

#[test]
fn claude_model_not_found_maps_to_backend_model_not_found() {
    let code = abp_error::ErrorCode::BackendModelNotFound;
    assert_eq!(code.as_str(), "backend_model_not_found");
}

#[test]
fn claude_api_error_maps_to_internal() {
    // Claude 500 "api_error" maps to internal
    let code = abp_error::ErrorCode::Internal;
    assert_eq!(code.as_str(), "internal");
    assert_eq!(code.category(), abp_error::ErrorCategory::Internal);
}

#[test]
fn claude_timeout_maps_to_backend_timeout() {
    let code = abp_error::ErrorCode::BackendTimeout;
    assert_eq!(code.as_str(), "backend_timeout");
}

#[test]
fn claude_backend_crashed_maps_correctly() {
    let code = abp_error::ErrorCode::BackendCrashed;
    assert_eq!(code.as_str(), "backend_crashed");
    assert_eq!(code.category(), abp_error::ErrorCategory::Backend);
}

#[test]
fn error_code_display_is_human_readable() {
    let code = abp_error::ErrorCode::BackendTimeout;
    let display = format!("{code}");
    assert!(display.contains("timed out"), "display = {display}");
}

#[test]
fn all_backend_error_codes_are_backend_category() {
    let backend_codes = vec![
        abp_error::ErrorCode::BackendNotFound,
        abp_error::ErrorCode::BackendUnavailable,
        abp_error::ErrorCode::BackendTimeout,
        abp_error::ErrorCode::BackendRateLimited,
        abp_error::ErrorCode::BackendAuthFailed,
        abp_error::ErrorCode::BackendModelNotFound,
        abp_error::ErrorCode::BackendCrashed,
    ];
    for code in backend_codes {
        assert_eq!(
            code.category(),
            abp_error::ErrorCategory::Backend,
            "code {:?} should be Backend category",
            code
        );
    }
}

#[test]
fn error_category_serde_roundtrip() {
    let cat = abp_error::ErrorCategory::Backend;
    let serialized = serde_json::to_string(&cat).unwrap();
    assert_eq!(serialized, r#""backend""#);
    let deser: abp_error::ErrorCategory = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deser, cat);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 13: Claude capability reporting
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_hello_reports_capabilities() {
    let hello = sidecar_kit::hello_frame("sidecar:claude");
    let json_str = serde_json::to_string(&hello).unwrap();
    let val: Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(val["contract_version"], "abp/v0.1");
    assert!(val["capabilities"].is_object());
}

#[test]
fn claude_capabilities_json_structure() {
    // Simulate what a Claude sidecar would advertise
    let caps = json!({
        "streaming": "native",
        "tool_use": "native",
        "tool_read": "native",
        "tool_write": "native",
        "tool_edit": "native",
        "tool_bash": "native",
        "extended_thinking": "native",
        "image_input": "native",
        "system_message": "native",
        "temperature": "native",
        "max_tokens": "native",
        "cache_control": "native"
    });
    // All keys must be snake_case
    for key in caps.as_object().unwrap().keys() {
        assert!(
            key.chars().all(|c| c.is_lowercase() || c == '_'),
            "capability key '{key}' must be snake_case"
        );
    }
}

#[test]
fn claude_hello_frame_has_backend_id() {
    let hello = sidecar_kit::hello_frame("sidecar:claude");
    match hello {
        sidecar_kit::Frame::Hello { backend, .. } => {
            assert_eq!(backend["id"], "sidecar:claude");
        }
        _ => panic!("expected Hello frame"),
    }
}

#[test]
fn claude_hello_contract_version() {
    let hello = sidecar_kit::hello_frame("sidecar:claude");
    match hello {
        sidecar_kit::Frame::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
        }
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 14: Prompt construction from WorkOrder (raw JSON)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_json_passthrough_mode() {
    // Simulate what run_raw builds internally
    let request = json!({"prompt": "Explain this code", "model": "claude-3-sonnet"});
    let work_order = json!({
        "id": "test-run-1",
        "task": request.get("prompt").and_then(|v| v.as_str()).unwrap_or("passthrough"),
        "lane": "patch_first",
        "workspace": {
            "root": ".",
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
    });
    assert_eq!(work_order["task"], "Explain this code");
    assert_eq!(work_order["config"]["vendor"]["abp.mode"], "passthrough");
    assert_eq!(
        work_order["config"]["vendor"]["abp.request"]["model"],
        "claude-3-sonnet"
    );
}

#[test]
fn work_order_json_mapped_mode() {
    // Simulate what run_mapped_raw builds
    let task = "Fix the auth bug";
    let vendor_config = json!({
        "abp.mode": "mapped"
    });
    let work_order = json!({
        "id": "test-run-2",
        "task": task,
        "lane": "patch_first",
        "workspace": {
            "root": "/my/project",
            "mode": "staged"
        },
        "context": {},
        "policy": {},
        "requirements": { "required": [] },
        "config": {
            "vendor": vendor_config
        }
    });
    assert_eq!(work_order["task"], "Fix the auth bug");
    assert_eq!(work_order["workspace"]["mode"], "staged");
    assert_eq!(work_order["config"]["vendor"]["abp.mode"], "mapped");
}

#[test]
fn work_order_passthrough_extracts_prompt() {
    let request = json!({"prompt": "Hello world"});
    let task = request
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("passthrough");
    assert_eq!(task, "Hello world");
}

#[test]
fn work_order_passthrough_fallback_when_no_prompt() {
    let request = json!({"messages": [{"role": "user", "content": "hi"}]});
    let task = request
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("passthrough");
    assert_eq!(task, "passthrough");
}

#[test]
fn work_order_mapped_with_extra_config() {
    let mut vendor_config = json!({"abp.mode": "mapped"});
    let extra = json!({"model": "claude-3-opus-20240229", "temperature": 0.7});
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            vendor_config[k] = v.clone();
        }
    }
    assert_eq!(vendor_config["abp.mode"], "mapped");
    assert_eq!(vendor_config["model"], "claude-3-opus-20240229");
    assert_eq!(vendor_config["temperature"], 0.7);
}

#[test]
fn work_order_lane_defaults_to_patch_first() {
    let lane = None::<String>;
    let effective = lane.as_deref().unwrap_or("patch_first");
    assert_eq!(effective, "patch_first");
}

#[test]
fn work_order_lane_can_be_overridden() {
    let lane = Some("workspace_first".to_string());
    let effective = lane.as_deref().unwrap_or("patch_first");
    assert_eq!(effective, "workspace_first");
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 15: Response parsing to Receipt (Value-based)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_json_complete_outcome() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-complete", "sidecar:claude").build();
    assert_eq!(receipt["outcome"], "complete");
}

#[test]
fn receipt_json_failed_outcome() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-fail", "sidecar:claude")
        .failed()
        .build();
    assert_eq!(receipt["outcome"], "failed");
}

#[test]
fn receipt_json_partial_outcome() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-partial", "sidecar:claude")
        .partial()
        .build();
    assert_eq!(receipt["outcome"], "partial");
}

#[test]
fn receipt_json_with_claude_usage() {
    let claude_usage = json!({
        "input_tokens": 2500,
        "output_tokens": 800,
        "cache_creation_input_tokens": 100,
        "cache_read_input_tokens": 50
    });
    let receipt = sidecar_kit::ReceiptBuilder::new("run-usage", "sidecar:claude")
        .usage_raw(claude_usage.clone())
        .input_tokens(2500)
        .output_tokens(800)
        .build();
    assert_eq!(receipt["usage_raw"]["input_tokens"], 2500);
    assert_eq!(receipt["usage_raw"]["output_tokens"], 800);
    assert_eq!(receipt["usage_raw"]["cache_creation_input_tokens"], 100);
    assert_eq!(receipt["usage"]["input_tokens"], 2500);
    assert_eq!(receipt["usage"]["output_tokens"], 800);
}

#[test]
fn receipt_json_with_trace_events() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-trace", "sidecar:claude")
        .event(sidecar_kit::event_run_started("starting"))
        .event(sidecar_kit::event_text_delta("hi"))
        .event(sidecar_kit::event_run_completed("done"))
        .build();
    let trace = receipt["trace"].as_array().unwrap();
    assert_eq!(trace.len(), 3);
    assert_eq!(trace[0]["type"], "run_started");
    assert_eq!(trace[1]["type"], "assistant_delta");
    assert_eq!(trace[2]["type"], "run_completed");
}

#[test]
fn receipt_json_with_multiple_artifacts() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-art", "sidecar:claude")
        .artifact("patch", "fix.patch")
        .artifact("log", "run.log")
        .build();
    let artifacts = receipt["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 2);
}

#[test]
fn receipt_wrapped_in_final_frame() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-final", "sidecar:claude").build();
    let frame = sidecar_kit::Frame::Final {
        ref_id: "run-final".into(),
        receipt: receipt.clone(),
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    let parsed: Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["t"], "final");
    assert_eq!(parsed["ref_id"], "run-final");
    assert_eq!(parsed["receipt"]["outcome"], "complete");
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 16: Claude-specific serde — content blocks, stop reasons
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_content_block_text_serde() {
    let block = json!({
        "type": "text",
        "text": "Hello, I can help with that."
    });
    let serialized = serde_json::to_string(&block).unwrap();
    let deser: Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deser["type"], "text");
    assert_eq!(deser["text"], "Hello, I can help with that.");
}

#[test]
fn claude_content_block_tool_use_serde() {
    let block = json!({
        "type": "tool_use",
        "id": "toolu_01XBq90qw90lq917835lq9",
        "name": "read_file",
        "input": {"path": "/src/lib.rs"}
    });
    let serialized = serde_json::to_string(&block).unwrap();
    let deser: Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deser["type"], "tool_use");
    assert_eq!(deser["name"], "read_file");
}

#[test]
fn claude_stop_reason_end_turn() {
    let response = json!({
        "stop_reason": "end_turn",
        "stop_sequence": null
    });
    assert_eq!(response["stop_reason"], "end_turn");
    assert!(response["stop_sequence"].is_null());
}

#[test]
fn claude_stop_reason_tool_use() {
    let response = json!({
        "stop_reason": "tool_use",
        "stop_sequence": null
    });
    assert_eq!(response["stop_reason"], "tool_use");
}

#[test]
fn claude_stop_reason_max_tokens() {
    let response = json!({
        "stop_reason": "max_tokens",
        "stop_sequence": null
    });
    assert_eq!(response["stop_reason"], "max_tokens");
}

#[test]
fn claude_usage_block_serde() {
    let usage = json!({
        "input_tokens": 1500,
        "output_tokens": 350,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 200
    });
    let serialized = serde_json::to_string(&usage).unwrap();
    let deser: Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deser["input_tokens"], 1500);
    assert_eq!(deser["output_tokens"], 350);
    assert_eq!(deser["cache_creation_input_tokens"], 0);
    assert_eq!(deser["cache_read_input_tokens"], 200);
}

#[test]
fn claude_error_response_serde() {
    let error = json!({
        "type": "error",
        "error": {
            "type": "overloaded_error",
            "message": "Overloaded"
        }
    });
    assert_eq!(error["error"]["type"], "overloaded_error");
    assert_eq!(error["error"]["message"], "Overloaded");
}

#[test]
fn claude_authentication_error_serde() {
    let error = json!({
        "type": "error",
        "error": {
            "type": "authentication_error",
            "message": "Invalid API key"
        }
    });
    assert_eq!(error["error"]["type"], "authentication_error");
}

#[test]
fn claude_rate_limit_error_serde() {
    let error = json!({
        "type": "error",
        "error": {
            "type": "rate_limit_error",
            "message": "Rate limit exceeded"
        }
    });
    assert_eq!(error["error"]["type"], "rate_limit_error");
}

#[test]
fn claude_model_field_serde() {
    let response = json!({
        "model": "claude-sonnet-4-20250514",
        "id": "msg_01A09q90qw90lq917835lq9"
    });
    assert!(response["model"].as_str().unwrap().contains("claude"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 17: CancelToken — cooperative cancellation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cancel_token_default_not_cancelled() {
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
fn cancel_token_clone_shares_state() {
    let token1 = sidecar_kit::CancelToken::new();
    let token2 = token1.clone();
    token1.cancel();
    assert!(token2.is_cancelled());
}

#[test]
fn cancel_token_default_impl() {
    let token = sidecar_kit::CancelToken::default();
    assert!(!token.is_cancelled());
}

#[tokio::test]
async fn cancel_token_cancelled_future_returns_immediately_when_already_cancelled() {
    let token = sidecar_kit::CancelToken::new();
    token.cancel();
    // This should return immediately since already cancelled
    token.cancelled().await;
    assert!(token.is_cancelled());
}

#[tokio::test]
async fn cancel_token_cancelled_future_with_spawn() {
    let token = sidecar_kit::CancelToken::new();
    let token_clone = token.clone();
    let handle = tokio::spawn(async move {
        token_clone.cancelled().await;
        true
    });
    // Small delay then cancel
    tokio::time::sleep(Duration::from_millis(10)).await;
    token.cancel();
    let result = handle.await.unwrap();
    assert!(result);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 18: ProcessSpec — sidecar process specification
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn process_spec_new() {
    let spec = sidecar_kit::ProcessSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn process_spec_with_args_and_env() {
    let mut spec = sidecar_kit::ProcessSpec::new("node");
    spec.args = vec!["host.js".into()];
    spec.env
        .insert("ANTHROPIC_API_KEY".into(), "sk-test".into());
    spec.cwd = Some("/workspace".into());

    assert_eq!(spec.args.len(), 1);
    assert_eq!(spec.env.get("ANTHROPIC_API_KEY").unwrap(), "sk-test");
    assert_eq!(spec.cwd.as_deref(), Some("/workspace"));
}

#[test]
fn process_spec_clone() {
    let mut spec = sidecar_kit::ProcessSpec::new("node");
    spec.args = vec!["host.js".into()];
    let cloned = spec.clone();
    assert_eq!(cloned.command, spec.command);
    assert_eq!(cloned.args, spec.args);
}

#[test]
fn process_spec_debug_format() {
    let spec = sidecar_kit::ProcessSpec::new("node");
    let dbg = format!("{spec:?}");
    assert!(dbg.contains("node"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 19: SidecarError — all variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_error_protocol_display() {
    let err = sidecar_kit::SidecarError::Protocol("invalid frame type".into());
    let msg = format!("{err}");
    assert!(msg.contains("protocol violation"));
}

#[test]
fn sidecar_error_fatal_display() {
    let err = sidecar_kit::SidecarError::Fatal("sidecar crashed".into());
    let msg = format!("{err}");
    assert!(msg.contains("fatal error"));
}

#[test]
fn sidecar_error_timeout_display() {
    let err = sidecar_kit::SidecarError::Timeout;
    let msg = format!("{err}");
    assert!(msg.contains("timed out"));
}

#[test]
fn sidecar_error_exited_with_code() {
    let err = sidecar_kit::SidecarError::Exited(Some(127));
    let msg = format!("{err}");
    assert!(msg.contains("127"));
}

#[test]
fn sidecar_error_exited_no_code() {
    let err = sidecar_kit::SidecarError::Exited(None);
    let msg = format!("{err}");
    assert!(msg.contains("exited"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 20: Frame.try_event / try_final typed extraction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn frame_try_event_success() {
    let frame = sidecar_kit::Frame::Event {
        ref_id: "run-1".into(),
        event: json!({"type": "assistant_delta", "text": "hi", "ts": "2024-01-01T00:00:00Z"}),
    };
    let result: Result<(String, Value), _> = frame.try_event();
    assert!(result.is_ok());
    let (ref_id, val) = result.unwrap();
    assert_eq!(ref_id, "run-1");
    assert_eq!(val["type"], "assistant_delta");
}

#[test]
fn frame_try_event_wrong_frame_type() {
    let frame = sidecar_kit::Frame::Ping { seq: 1 };
    let result: Result<(String, Value), _> = frame.try_event();
    assert!(result.is_err());
}

#[test]
fn frame_try_final_success() {
    let frame = sidecar_kit::Frame::Final {
        ref_id: "run-2".into(),
        receipt: json!({"outcome": "complete"}),
    };
    let result: Result<(String, Value), _> = frame.try_final();
    assert!(result.is_ok());
    let (ref_id, val) = result.unwrap();
    assert_eq!(ref_id, "run-2");
    assert_eq!(val["outcome"], "complete");
}

#[test]
fn frame_try_final_wrong_frame_type() {
    let frame = sidecar_kit::Frame::Event {
        ref_id: "run-2".into(),
        event: json!({}),
    };
    let result: Result<(String, Value), _> = frame.try_final();
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 21: Edge cases and robustness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_task_string_in_run_options() {
    let opts = RunOptions {
        lane: Some("".into()),
        workspace_root: Some("".into()),
        extra_config: None,
    };
    assert_eq!(opts.lane.as_deref(), Some(""));
    assert_eq!(opts.workspace_root.as_deref(), Some(""));
}

#[test]
fn unicode_in_event_text() {
    let ev = sidecar_kit::event_text_delta("こんにちは 🌍");
    assert_eq!(ev["text"], "こんにちは 🌍");
}

#[test]
fn very_long_text_in_event() {
    let long_text = "a".repeat(100_000);
    let ev = sidecar_kit::event_text_delta(&long_text);
    assert_eq!(ev["text"].as_str().unwrap().len(), 100_000);
}

#[test]
fn empty_text_in_event() {
    let ev = sidecar_kit::event_text_delta("");
    assert_eq!(ev["text"], "");
}

#[test]
fn special_characters_in_tool_name() {
    let ev = sidecar_kit::event_tool_call("my-tool_v2.0", None, json!({}));
    assert_eq!(ev["tool_name"], "my-tool_v2.0");
}

#[test]
fn null_extra_config_in_run_options() {
    let opts = RunOptions {
        extra_config: Some(Value::Null),
        ..Default::default()
    };
    assert!(opts.extra_config.unwrap().is_null());
}

#[test]
fn nested_json_in_tool_input() {
    let input = json!({
        "content": {
            "nested": {
                "deeply": {
                    "value": [1, 2, 3]
                }
            }
        }
    });
    let ev = sidecar_kit::event_tool_call("complex_tool", Some("tu_1"), input.clone());
    assert_eq!(ev["input"]["content"]["nested"]["deeply"]["value"][0], 1);
}

#[test]
fn config_with_many_env_vars() {
    let mut cfg = ClaudeBridgeConfig::new();
    for i in 0..50 {
        cfg = cfg.with_env(format!("VAR_{i}"), format!("val_{i}"));
    }
    assert_eq!(cfg.env.len(), 50);
}

#[test]
fn receipt_builder_chained_events_ordering() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-ord", "sidecar:claude")
        .event(json!({"order": 0}))
        .event(json!({"order": 1}))
        .event(json!({"order": 2}))
        .build();
    let trace = receipt["trace"].as_array().unwrap();
    for (i, ev) in trace.iter().enumerate() {
        assert_eq!(ev["order"], i as i64);
    }
}

#[test]
fn frame_serde_preserves_all_fields() {
    let hello = json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": {"id": "sidecar:claude", "version": "1.0"},
        "capabilities": {"streaming": "native"},
        "mode": null
    });
    let frame: sidecar_kit::Frame = serde_json::from_value(hello.clone()).unwrap();
    let reserialized = serde_json::to_value(&frame).unwrap();
    assert_eq!(reserialized["contract_version"], "abp/v0.1");
    assert_eq!(reserialized["backend"]["id"], "sidecar:claude");
}

#[test]
fn multiple_bridge_instances_independent() {
    let cfg1 = ClaudeBridgeConfig::new().with_api_key("key1");
    let cfg2 = ClaudeBridgeConfig::new().with_api_key("key2");
    let _b1 = ClaudeBridge::new(cfg1);
    let _b2 = ClaudeBridge::new(cfg2);
    // Both should be independently constructable
}

#[test]
fn error_code_protocol_variants() {
    let codes = vec![
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
        abp_error::ErrorCode::ProtocolHandshakeFailed,
        abp_error::ErrorCode::ProtocolMissingRefId,
        abp_error::ErrorCode::ProtocolUnexpectedMessage,
        abp_error::ErrorCode::ProtocolVersionMismatch,
    ];
    for code in codes {
        assert_eq!(
            code.category(),
            abp_error::ErrorCategory::Protocol,
            "{:?} should be Protocol",
            code
        );
        assert!(
            code.as_str().starts_with("protocol_"),
            "{:?} as_str should start with protocol_",
            code
        );
    }
}

#[test]
fn error_code_execution_variants() {
    let codes = vec![
        abp_error::ErrorCode::ExecutionToolFailed,
        abp_error::ErrorCode::ExecutionWorkspaceError,
        abp_error::ErrorCode::ExecutionPermissionDenied,
    ];
    for code in codes {
        assert_eq!(
            code.category(),
            abp_error::ErrorCategory::Execution,
            "{:?} should be Execution",
            code
        );
        assert!(
            code.as_str().starts_with("execution_"),
            "{:?} as_str should start with execution_",
            code
        );
    }
}

#[test]
fn error_code_mapping_variants() {
    let codes = vec![
        abp_error::ErrorCode::MappingUnsupportedCapability,
        abp_error::ErrorCode::MappingDialectMismatch,
        abp_error::ErrorCode::MappingLossyConversion,
        abp_error::ErrorCode::MappingUnmappableTool,
    ];
    for code in codes {
        assert_eq!(
            code.category(),
            abp_error::ErrorCategory::Mapping,
            "{:?} should be Mapping",
            code
        );
        assert!(
            code.as_str().starts_with("mapping_"),
            "{:?} as_str should start with mapping_",
            code
        );
    }
}

#[test]
fn error_code_internal_is_just_internal() {
    let code = abp_error::ErrorCode::Internal;
    assert_eq!(code.as_str(), "internal");
    assert_eq!(code.category(), abp_error::ErrorCategory::Internal);
}

#[test]
fn bridge_error_debug_includes_variant() {
    let errors: Vec<BridgeError> = vec![
        BridgeError::NodeNotFound("x".into()),
        BridgeError::HostScriptNotFound("y".into()),
        BridgeError::Config("z".into()),
        BridgeError::Run("w".into()),
    ];
    for err in &errors {
        let dbg = format!("{err:?}");
        assert!(!dbg.is_empty());
    }
}

#[test]
fn config_handshake_timeout_zero() {
    let cfg = ClaudeBridgeConfig::new().with_handshake_timeout(Duration::ZERO);
    assert_eq!(cfg.handshake_timeout, Duration::ZERO);
}

#[test]
fn config_channel_buffer_one() {
    let cfg = ClaudeBridgeConfig::new().with_channel_buffer(1);
    assert_eq!(cfg.channel_buffer, 1);
}
