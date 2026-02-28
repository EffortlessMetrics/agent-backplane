use std::path::{Path, PathBuf};
use std::time::Duration;

use claude_bridge::config::ClaudeBridgeConfig;
use claude_bridge::discovery::{
    resolve_host_script, resolve_node, DEFAULT_NODE_COMMAND, HOST_SCRIPT_ENV, HOST_SCRIPT_RELATIVE,
};
use claude_bridge::error::BridgeError;
use claude_bridge::raw::RunOptions;

// ── Config defaults ────────────────────────────────────────────────

#[test]
fn config_default_values() {
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
fn config_new_equals_default() {
    let a = ClaudeBridgeConfig::new();
    let b = ClaudeBridgeConfig::default();
    assert_eq!(a.handshake_timeout, b.handshake_timeout);
    assert_eq!(a.channel_buffer, b.channel_buffer);
}

// ── Config builder ─────────────────────────────────────────────────

#[test]
fn config_builder_with_api_key() {
    let cfg = ClaudeBridgeConfig::new().with_api_key("sk-test-key");
    assert_eq!(cfg.env.get("ANTHROPIC_API_KEY").unwrap(), "sk-test-key");
}

#[test]
fn config_builder_with_host_script() {
    let cfg = ClaudeBridgeConfig::new().with_host_script("/tmp/host.js");
    assert_eq!(cfg.host_script, Some(PathBuf::from("/tmp/host.js")));
}

#[test]
fn config_builder_with_cwd() {
    let cfg = ClaudeBridgeConfig::new().with_cwd("/my/project");
    assert_eq!(cfg.cwd, Some(PathBuf::from("/my/project")));
}

#[test]
fn config_builder_with_adapter_module() {
    let cfg = ClaudeBridgeConfig::new().with_adapter_module("/adapters/custom.js");
    assert_eq!(cfg.adapter_module, Some(PathBuf::from("/adapters/custom.js")));
}

#[test]
fn config_builder_with_node_command() {
    let cfg = ClaudeBridgeConfig::new().with_node_command("node18");
    assert_eq!(cfg.node_command, Some("node18".to_string()));
}

#[test]
fn config_builder_with_env() {
    let cfg = ClaudeBridgeConfig::new()
        .with_env("FOO", "bar")
        .with_env("BAZ", "qux");
    assert_eq!(cfg.env.get("FOO").unwrap(), "bar");
    assert_eq!(cfg.env.get("BAZ").unwrap(), "qux");
}

#[test]
fn config_builder_with_handshake_timeout() {
    let cfg = ClaudeBridgeConfig::new().with_handshake_timeout(Duration::from_secs(60));
    assert_eq!(cfg.handshake_timeout, Duration::from_secs(60));
}

#[test]
fn config_builder_with_channel_buffer() {
    let cfg = ClaudeBridgeConfig::new().with_channel_buffer(512);
    assert_eq!(cfg.channel_buffer, 512);
}

#[test]
fn config_builder_chaining() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("key")
        .with_cwd("/project")
        .with_channel_buffer(128)
        .with_handshake_timeout(Duration::from_secs(10));

    assert_eq!(cfg.env.get("ANTHROPIC_API_KEY").unwrap(), "key");
    assert_eq!(cfg.cwd, Some(PathBuf::from("/project")));
    assert_eq!(cfg.channel_buffer, 128);
    assert_eq!(cfg.handshake_timeout, Duration::from_secs(10));
}

// ── Error Display ──────────────────────────────────────────────────

#[test]
fn error_display_node_not_found() {
    let err = BridgeError::NodeNotFound("node18 missing".to_string());
    assert_eq!(err.to_string(), "node.js not found: node18 missing");
}

#[test]
fn error_display_host_script_not_found() {
    let err = BridgeError::HostScriptNotFound("no host.js".to_string());
    assert_eq!(err.to_string(), "host script not found: no host.js");
}

#[test]
fn error_display_config() {
    let err = BridgeError::Config("bad value".to_string());
    assert_eq!(err.to_string(), "configuration error: bad value");
}

#[test]
fn error_display_run() {
    let err = BridgeError::Run("timeout".to_string());
    assert_eq!(err.to_string(), "run error: timeout");
}

#[test]
fn error_is_debug() {
    let err = BridgeError::Config("test".to_string());
    let debug = format!("{:?}", err);
    assert!(debug.contains("Config"));
}

// ── Discovery constants ────────────────────────────────────────────

#[test]
fn discovery_constants() {
    assert_eq!(DEFAULT_NODE_COMMAND, "node");
    assert_eq!(HOST_SCRIPT_RELATIVE, "hosts/claude/host.js");
    assert_eq!(HOST_SCRIPT_ENV, "ABP_CLAUDE_HOST_SCRIPT");
}

// ── Discovery: resolve_host_script ─────────────────────────────────

#[test]
fn resolve_host_script_explicit_nonexistent() {
    let path = Path::new("/nonexistent/path/to/host.js");
    let result = resolve_host_script(Some(path));
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("host script not found"), "got: {msg}");
    assert!(msg.contains("nonexistent"), "got: {msg}");
}

#[test]
fn resolve_host_script_none_falls_through() {
    // With no explicit path and no env var override, it may or may not find
    // the script depending on the test environment. We just verify it doesn't
    // panic and returns a Result.
    let _result = resolve_host_script(None);
}

// ── Discovery: resolve_node ────────────────────────────────────────

#[test]
fn resolve_node_explicit_nonexistent() {
    let result = resolve_node(Some("/nonexistent/node-binary"));
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("not found"), "got: {msg}");
}

#[test]
fn resolve_node_empty_string_fallback() {
    // Empty string should be treated as no override and fall back to default.
    let result = resolve_node(Some(""));
    // Either finds "node" on PATH or returns NodeNotFound
    match result {
        Ok(cmd) => assert_eq!(cmd, "node"),
        Err(e) => assert!(e.to_string().contains("not found")),
    }
}

#[test]
fn resolve_node_whitespace_only_fallback() {
    let result = resolve_node(Some("   "));
    match result {
        Ok(cmd) => assert_eq!(cmd, "node"),
        Err(e) => assert!(e.to_string().contains("not found")),
    }
}

#[test]
fn resolve_node_none_uses_default() {
    let result = resolve_node(None);
    match result {
        Ok(cmd) => assert_eq!(cmd, "node"),
        Err(e) => assert!(e.to_string().contains("not found")),
    }
}

// ── RunOptions defaults ────────────────────────────────────────────

#[test]
fn run_options_default() {
    let opts = RunOptions::default();
    assert!(opts.lane.is_none());
    assert!(opts.workspace_root.is_none());
    assert!(opts.extra_config.is_none());
}

// ── Module re-exports ──────────────────────────────────────────────

#[test]
fn reexports_accessible() {
    // Verify key types are accessible from the crate root
    let _cfg = claude_bridge::ClaudeBridgeConfig::new();
    let _opts = claude_bridge::RunOptions::default();
    let _err = claude_bridge::BridgeError::Config("test".into());

    // ClaudeBridge struct is accessible
    let cfg = claude_bridge::ClaudeBridgeConfig::new();
    let _bridge = claude_bridge::ClaudeBridge::new(cfg);
}
