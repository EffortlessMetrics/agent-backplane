// SPDX-License-Identifier: MIT OR Apache-2.0
//! Round-trip and edge-case tests for claude-bridge types.

use std::path::PathBuf;
use std::time::Duration;

use claude_bridge::config::ClaudeBridgeConfig;
use claude_bridge::discovery;
use claude_bridge::error::BridgeError;
use claude_bridge::raw::RunOptions;

// ── Config round-trip through Clone ────────────────────────────────

#[test]
fn config_clone_preserves_all_fields() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("sk-key")
        .with_host_script("/path/to/host.js")
        .with_cwd("/workspace")
        .with_adapter_module("/adapter.js")
        .with_node_command("node20")
        .with_env("EXTRA", "val")
        .with_handshake_timeout(Duration::from_millis(500))
        .with_channel_buffer(64);

    let cloned = cfg.clone();

    assert_eq!(cloned.node_command, Some("node20".into()));
    assert_eq!(cloned.host_script, Some(PathBuf::from("/path/to/host.js")));
    assert_eq!(cloned.cwd, Some(PathBuf::from("/workspace")));
    assert_eq!(cloned.adapter_module, Some(PathBuf::from("/adapter.js")));
    assert_eq!(cloned.env.get("ANTHROPIC_API_KEY").unwrap(), "sk-key");
    assert_eq!(cloned.env.get("EXTRA").unwrap(), "val");
    assert_eq!(cloned.handshake_timeout, Duration::from_millis(500));
    assert_eq!(cloned.channel_buffer, 64);
}

// ── Config builder overwrite semantics ─────────────────────────────

#[test]
fn config_api_key_overwrite() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("first")
        .with_api_key("second");
    assert_eq!(cfg.env.get("ANTHROPIC_API_KEY").unwrap(), "second");
}

#[test]
fn config_env_overwrite_same_key() {
    let cfg = ClaudeBridgeConfig::new()
        .with_env("KEY", "old")
        .with_env("KEY", "new");
    assert_eq!(cfg.env.get("KEY").unwrap(), "new");
    assert_eq!(cfg.env.len(), 1);
}

#[test]
fn config_host_script_overwrite() {
    let cfg = ClaudeBridgeConfig::new()
        .with_host_script("/first.js")
        .with_host_script("/second.js");
    assert_eq!(cfg.host_script, Some(PathBuf::from("/second.js")));
}

#[test]
fn config_node_command_overwrite() {
    let cfg = ClaudeBridgeConfig::new()
        .with_node_command("node16")
        .with_node_command("node20");
    assert_eq!(cfg.node_command, Some("node20".into()));
}

// ── Empty / edge-case configs ──────────────────────────────────────

#[test]
fn config_empty_string_api_key() {
    let cfg = ClaudeBridgeConfig::new().with_api_key("");
    assert_eq!(cfg.env.get("ANTHROPIC_API_KEY").unwrap(), "");
}

#[test]
fn config_empty_string_env_key() {
    let cfg = ClaudeBridgeConfig::new().with_env("", "value");
    assert_eq!(cfg.env.get("").unwrap(), "value");
}

#[test]
fn config_zero_channel_buffer() {
    let cfg = ClaudeBridgeConfig::new().with_channel_buffer(0);
    assert_eq!(cfg.channel_buffer, 0);
}

#[test]
fn config_zero_duration_timeout() {
    let cfg = ClaudeBridgeConfig::new().with_handshake_timeout(Duration::ZERO);
    assert_eq!(cfg.handshake_timeout, Duration::ZERO);
}

#[test]
fn config_very_large_channel_buffer() {
    let cfg = ClaudeBridgeConfig::new().with_channel_buffer(usize::MAX);
    assert_eq!(cfg.channel_buffer, usize::MAX);
}

#[test]
fn config_debug_impl() {
    let cfg = ClaudeBridgeConfig::new().with_api_key("key123");
    let debug = format!("{:?}", cfg);
    assert!(debug.contains("ClaudeBridgeConfig"));
    assert!(debug.contains("ANTHROPIC_API_KEY"));
}

// ── Error display messages ─────────────────────────────────────────

#[test]
fn error_node_not_found_contains_detail() {
    let err = BridgeError::NodeNotFound("custom-node v99".into());
    let msg = err.to_string();
    assert!(msg.contains("node.js not found"));
    assert!(msg.contains("custom-node v99"));
}

#[test]
fn error_host_script_not_found_contains_path() {
    let err = BridgeError::HostScriptNotFound("/missing/host.js".into());
    let msg = err.to_string();
    assert!(msg.contains("host script not found"));
    assert!(msg.contains("/missing/host.js"));
}

#[test]
fn error_config_contains_detail() {
    let err = BridgeError::Config("invalid timeout value".into());
    let msg = err.to_string();
    assert!(msg.contains("configuration error"));
    assert!(msg.contains("invalid timeout value"));
}

#[test]
fn error_run_contains_detail() {
    let err = BridgeError::Run("process crashed".into());
    let msg = err.to_string();
    assert!(msg.contains("run error"));
    assert!(msg.contains("process crashed"));
}

#[test]
fn error_sidecar_from_conversion() {
    let sidecar_err = sidecar_kit::SidecarError::Timeout;
    let bridge_err: BridgeError = sidecar_err.into();
    let msg = bridge_err.to_string();
    assert!(msg.contains("sidecar error"));
    assert!(msg.contains("timed out"));
}

#[test]
fn error_sidecar_protocol_variant() {
    let sidecar_err = sidecar_kit::SidecarError::Protocol("bad envelope".into());
    let bridge_err: BridgeError = bridge_err_from_sidecar(sidecar_err);
    assert!(bridge_err.to_string().contains("protocol violation"));
}

fn bridge_err_from_sidecar(e: sidecar_kit::SidecarError) -> BridgeError {
    BridgeError::from(e)
}

#[test]
fn error_all_variants_non_empty_display() {
    let variants: Vec<BridgeError> = vec![
        BridgeError::NodeNotFound("x".into()),
        BridgeError::HostScriptNotFound("x".into()),
        BridgeError::Config("x".into()),
        BridgeError::Run("x".into()),
        BridgeError::Sidecar(sidecar_kit::SidecarError::Timeout),
    ];
    for err in &variants {
        assert!(!err.to_string().is_empty(), "empty display for {:?}", err);
    }
}

#[test]
fn error_debug_all_variants() {
    let variants: Vec<BridgeError> = vec![
        BridgeError::NodeNotFound("n".into()),
        BridgeError::HostScriptNotFound("h".into()),
        BridgeError::Config("c".into()),
        BridgeError::Run("r".into()),
    ];
    for err in &variants {
        let debug = format!("{:?}", err);
        assert!(!debug.is_empty());
    }
}

// ── RunOptions ─────────────────────────────────────────────────────

#[test]
fn run_options_all_none_by_default() {
    let opts = RunOptions::default();
    assert!(opts.lane.is_none());
    assert!(opts.workspace_root.is_none());
    assert!(opts.extra_config.is_none());
}

#[test]
fn run_options_with_values() {
    let opts = RunOptions {
        lane: Some("review".into()),
        workspace_root: Some("/project".into()),
        extra_config: Some(serde_json::json!({"model": "claude-3"})),
    };
    assert_eq!(opts.lane.as_deref(), Some("review"));
    assert_eq!(opts.workspace_root.as_deref(), Some("/project"));
    assert!(opts.extra_config.unwrap().is_object());
}

#[test]
fn run_options_clone() {
    let opts = RunOptions {
        lane: Some("patch".into()),
        workspace_root: Some("/ws".into()),
        extra_config: Some(serde_json::json!(42)),
    };
    let cloned = opts.clone();
    assert_eq!(cloned.lane, Some("patch".into()));
    assert_eq!(cloned.workspace_root, Some("/ws".into()));
    assert_eq!(cloned.extra_config, Some(serde_json::json!(42)));
}

#[test]
fn run_options_debug() {
    let opts = RunOptions::default();
    let debug = format!("{:?}", opts);
    assert!(debug.contains("RunOptions"));
}

// ── Discovery edge cases ───────────────────────────────────────────

#[test]
fn resolve_host_script_with_existing_tempfile() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("host.js");
    std::fs::write(&script, "// host").unwrap();

    let result = discovery::resolve_host_script(Some(&script));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), script);
}

#[test]
fn resolve_host_script_explicit_directory_fails() {
    let dir = tempfile::tempdir().unwrap();
    // Passing a directory, not a file — should fail
    let result = discovery::resolve_host_script(Some(dir.path()));
    assert!(result.is_err());
}

#[test]
fn resolve_node_with_absolute_path_to_nonexistent() {
    let result = discovery::resolve_node(Some("/no/such/binary/node-xyz"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

// ── ClaudeBridge construction ──────────────────────────────────────

#[test]
fn bridge_new_does_not_panic() {
    let cfg = ClaudeBridgeConfig::new();
    let _bridge = claude_bridge::ClaudeBridge::new(cfg);
}

#[test]
fn bridge_with_full_config() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("test-key")
        .with_node_command("node")
        .with_channel_buffer(32)
        .with_handshake_timeout(Duration::from_secs(5));
    let _bridge = claude_bridge::ClaudeBridge::new(cfg);
}
