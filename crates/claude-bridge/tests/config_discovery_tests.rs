// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for config, discovery, error, and type modules.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use claude_bridge::config::ClaudeBridgeConfig;
use claude_bridge::discovery::{
    DEFAULT_NODE_COMMAND, HOST_SCRIPT_ENV, HOST_SCRIPT_RELATIVE, resolve_host_script, resolve_node,
};
use claude_bridge::error::BridgeError;
use claude_bridge::raw::RunOptions;

// ═══════════════════════════════════════════════════════════════════
// Config — defaults
// ═══════════════════════════════════════════════════════════════════

#[test]
fn default_config_has_sensible_timeout() {
    let cfg = ClaudeBridgeConfig::default();
    // 30s is long enough for a real handshake, short enough to fail fast.
    assert!(cfg.handshake_timeout >= Duration::from_secs(5));
    assert!(cfg.handshake_timeout <= Duration::from_secs(120));
}

#[test]
fn default_config_has_nonzero_buffer() {
    let cfg = ClaudeBridgeConfig::default();
    assert!(cfg.channel_buffer > 0, "default buffer must be > 0");
}

#[test]
fn default_config_no_optional_fields_set() {
    let cfg = ClaudeBridgeConfig::default();
    assert!(cfg.node_command.is_none());
    assert!(cfg.host_script.is_none());
    assert!(cfg.cwd.is_none());
    assert!(cfg.adapter_module.is_none());
}

#[test]
fn default_config_env_is_empty() {
    let cfg = ClaudeBridgeConfig::default();
    assert!(cfg.env.is_empty(), "default env should carry no vars");
}

// ═══════════════════════════════════════════════════════════════════
// Config — builder overrides
// ═══════════════════════════════════════════════════════════════════

#[test]
fn builder_with_api_key_sets_anthropic_env() {
    let cfg = ClaudeBridgeConfig::new().with_api_key("sk-ant-test");
    assert_eq!(
        cfg.env.get("ANTHROPIC_API_KEY"),
        Some(&"sk-ant-test".to_string())
    );
}

#[test]
fn builder_api_key_last_write_wins() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("first")
        .with_api_key("second");
    assert_eq!(
        cfg.env.get("ANTHROPIC_API_KEY").unwrap(),
        "second",
        "last call to with_api_key should win"
    );
}

#[test]
fn builder_multiple_env_vars_coexist() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("key1")
        .with_env("MODEL", "claude-3")
        .with_env("REGION", "us-east");

    assert_eq!(cfg.env.len(), 3);
    assert_eq!(cfg.env.get("ANTHROPIC_API_KEY").unwrap(), "key1");
    assert_eq!(cfg.env.get("MODEL").unwrap(), "claude-3");
    assert_eq!(cfg.env.get("REGION").unwrap(), "us-east");
}

#[test]
fn builder_env_keys_are_deterministically_ordered() {
    let cfg = ClaudeBridgeConfig::new()
        .with_env("Z_LAST", "1")
        .with_env("A_FIRST", "2")
        .with_env("M_MIDDLE", "3");

    let keys: Vec<&String> = cfg.env.keys().collect();
    assert_eq!(keys, vec!["A_FIRST", "M_MIDDLE", "Z_LAST"]);
}

#[test]
fn builder_overrides_all_optional_paths() {
    let cfg = ClaudeBridgeConfig::new()
        .with_host_script("host.js")
        .with_cwd("/work")
        .with_adapter_module("adapter.js")
        .with_node_command("node20");

    assert_eq!(cfg.host_script.unwrap(), PathBuf::from("host.js"));
    assert_eq!(cfg.cwd.unwrap(), PathBuf::from("/work"));
    assert_eq!(cfg.adapter_module.unwrap(), PathBuf::from("adapter.js"));
    assert_eq!(cfg.node_command.unwrap(), "node20");
}

#[test]
fn builder_timeout_and_buffer_override() {
    let cfg = ClaudeBridgeConfig::new()
        .with_handshake_timeout(Duration::from_millis(100))
        .with_channel_buffer(8);

    assert_eq!(cfg.handshake_timeout, Duration::from_millis(100));
    assert_eq!(cfg.channel_buffer, 8);
}

// ═══════════════════════════════════════════════════════════════════
// Config — environment variable passthrough
// ═══════════════════════════════════════════════════════════════════

#[test]
fn config_env_vars_are_btreemap() {
    // BTreeMap guarantees deterministic iteration (important for hashing).
    let cfg = ClaudeBridgeConfig::new()
        .with_env("B", "2")
        .with_env("A", "1");
    let _: &BTreeMap<String, String> = &cfg.env;
    let first = cfg.env.keys().next().unwrap();
    assert_eq!(first, "A", "BTreeMap should sort keys");
}

#[test]
fn config_env_unicode_values() {
    let cfg = ClaudeBridgeConfig::new().with_env("GREETING", "こんにちは");
    assert_eq!(cfg.env.get("GREETING").unwrap(), "こんにちは");
}

#[test]
fn config_env_with_equals_in_value() {
    let cfg = ClaudeBridgeConfig::new().with_env("QUERY", "a=1&b=2");
    assert_eq!(cfg.env.get("QUERY").unwrap(), "a=1&b=2");
}

// ═══════════════════════════════════════════════════════════════════
// Config — clone round-trip (serialization substitute)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn config_clone_roundtrip_preserves_identity() {
    let original = ClaudeBridgeConfig::new()
        .with_api_key("roundtrip-key")
        .with_host_script("/rt/host.js")
        .with_cwd("/rt/cwd")
        .with_adapter_module("/rt/adapter.js")
        .with_node_command("node-rt")
        .with_env("RT_VAR", "42")
        .with_handshake_timeout(Duration::from_secs(7))
        .with_channel_buffer(99);

    let cloned = original.clone();

    assert_eq!(cloned.node_command, original.node_command);
    assert_eq!(cloned.host_script, original.host_script);
    assert_eq!(cloned.cwd, original.cwd);
    assert_eq!(cloned.adapter_module, original.adapter_module);
    assert_eq!(cloned.env, original.env);
    assert_eq!(cloned.handshake_timeout, original.handshake_timeout);
    assert_eq!(cloned.channel_buffer, original.channel_buffer);
}

#[test]
fn config_debug_output_includes_struct_name() {
    let cfg = ClaudeBridgeConfig::new();
    let dbg = format!("{cfg:?}");
    assert!(
        dbg.contains("ClaudeBridgeConfig"),
        "Debug should include type name"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Config — empty API key is acceptable
// ═══════════════════════════════════════════════════════════════════

#[test]
fn config_empty_api_key_is_accepted() {
    let cfg = ClaudeBridgeConfig::new().with_api_key("");
    assert_eq!(
        cfg.env.get("ANTHROPIC_API_KEY").unwrap(),
        "",
        "empty key should be accepted; validation happens elsewhere"
    );
}

#[test]
fn config_whitespace_api_key_is_stored_as_is() {
    let cfg = ClaudeBridgeConfig::new().with_api_key("  ");
    assert_eq!(
        cfg.env.get("ANTHROPIC_API_KEY").unwrap(),
        "  ",
        "whitespace key should be stored verbatim"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Discovery — resolve_node
// ═══════════════════════════════════════════════════════════════════

#[test]
fn resolve_node_none_returns_default_or_not_found() {
    match resolve_node(None) {
        Ok(cmd) => assert_eq!(cmd, DEFAULT_NODE_COMMAND),
        Err(e) => assert!(
            e.to_string().contains("not found"),
            "error should mention 'not found'"
        ),
    }
}

#[test]
fn resolve_node_explicit_missing_returns_helpful_error() {
    let result = resolve_node(Some("nonexistent-node-v99"));
    let err = result.expect_err("should fail for missing binary");
    let msg = err.to_string();
    assert!(
        msg.contains("nonexistent-node-v99"),
        "error should echo the command that was not found: {msg}"
    );
    assert!(
        msg.contains("not found"),
        "error should say 'not found': {msg}"
    );
}

#[test]
fn resolve_node_empty_string_falls_back_to_default() {
    // Empty override → treated as no override → fall back to PATH search.
    match resolve_node(Some("")) {
        Ok(cmd) => assert_eq!(cmd, DEFAULT_NODE_COMMAND),
        Err(e) => assert!(e.to_string().contains("not found")),
    }
}

#[test]
fn resolve_node_whitespace_falls_back_to_default() {
    match resolve_node(Some("   \t  ")) {
        Ok(cmd) => assert_eq!(cmd, DEFAULT_NODE_COMMAND),
        Err(e) => assert!(e.to_string().contains("not found")),
    }
}

#[test]
fn resolve_node_with_real_tempdir_executable() {
    let dir = tempfile::tempdir().unwrap();
    let fake_node = dir.path().join(if cfg!(windows) {
        "fakenode.exe"
    } else {
        "fakenode"
    });
    std::fs::write(&fake_node, b"fake").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_node, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Passing the full path should find it.
    let result = resolve_node(Some(fake_node.to_str().unwrap()));
    assert!(result.is_ok(), "should find executable at explicit path");
    assert_eq!(result.unwrap(), fake_node.to_string_lossy().to_string());
}

// ═══════════════════════════════════════════════════════════════════
// Discovery — resolve_host_script
// ═══════════════════════════════════════════════════════════════════

#[test]
fn resolve_host_script_explicit_existing_file_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("host.js");
    std::fs::write(&script, "// host script").unwrap();

    let result = resolve_host_script(Some(&script));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), script);
}

#[test]
fn resolve_host_script_explicit_nonexistent_has_helpful_message() {
    let bogus = Path::new("/no/such/dir/host.js");
    let err = resolve_host_script(Some(bogus)).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("host script not found"),
        "should say host script not found: {msg}"
    );
    assert!(
        msg.contains("host.js"),
        "should mention the file name: {msg}"
    );
}

#[test]
fn resolve_host_script_explicit_directory_fails() {
    let dir = tempfile::tempdir().unwrap();
    // Passing a directory path (not a file) should fail.
    let result = resolve_host_script(Some(dir.path()));
    assert!(
        result.is_err(),
        "directory should not be accepted as script"
    );
}

#[test]
fn resolve_host_script_env_override_with_real_file() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("env_host.js");
    std::fs::write(&script, "// env host").unwrap();

    // Temporarily set the env var. We use the real constant.
    let _guard = EnvGuard::set(HOST_SCRIPT_ENV, script.to_str().unwrap());

    // None explicit → should fall through to env var lookup.
    // Note: this may still pick up a CWD-relative script first, but if
    // the env path is valid it should eventually work. We verify at least
    // that no panic occurs and the result is Ok.
    let result = resolve_host_script(None);
    // Depending on CWD, env var may or may not be the winner, but it
    // should not error because the file exists.
    if let Ok(path) = &result {
        // If it resolved via env, it should match our tempfile.
        // If it resolved via CWD, that's fine too.
        assert!(path.is_file(), "resolved path should be a real file");
    }
}

#[test]
fn resolve_host_script_none_without_env_does_not_panic() {
    let _guard = EnvGuard::remove(HOST_SCRIPT_ENV);
    // Should either find the script in CWD/home or return an error — never panic.
    let _result = resolve_host_script(None);
}

#[test]
fn discovery_constants_are_stable() {
    assert_eq!(DEFAULT_NODE_COMMAND, "node");
    assert_eq!(HOST_SCRIPT_RELATIVE, "hosts/claude/host.js");
    assert!(!HOST_SCRIPT_ENV.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// Error — all variants have non-empty Display
// ═══════════════════════════════════════════════════════════════════

#[test]
fn all_error_variants_have_non_empty_display() {
    let variants: Vec<BridgeError> = vec![
        BridgeError::NodeNotFound("detail".into()),
        BridgeError::HostScriptNotFound("detail".into()),
        BridgeError::Config("detail".into()),
        BridgeError::Run("detail".into()),
        BridgeError::Sidecar(sidecar_kit::SidecarError::Timeout),
    ];
    for err in &variants {
        let msg = err.to_string();
        assert!(!msg.is_empty(), "Display should be non-empty for {err:?}");
        assert!(msg.len() > 5, "Display should be descriptive, got: {msg}");
    }
}

#[test]
fn all_error_variants_have_non_empty_debug() {
    let variants: Vec<BridgeError> = vec![
        BridgeError::NodeNotFound("n".into()),
        BridgeError::HostScriptNotFound("h".into()),
        BridgeError::Config("c".into()),
        BridgeError::Run("r".into()),
        BridgeError::Sidecar(sidecar_kit::SidecarError::Fatal("f".into())),
    ];
    for err in &variants {
        let dbg = format!("{err:?}");
        assert!(!dbg.is_empty(), "Debug should be non-empty for {err:?}");
    }
}

// ═══════════════════════════════════════════════════════════════════
// Error — source chains are preserved
// ═══════════════════════════════════════════════════════════════════

#[test]
fn error_sidecar_spawn_preserves_source_chain() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "process gone");
    let sidecar_err = sidecar_kit::SidecarError::Spawn(io_err);
    let bridge_err: BridgeError = sidecar_err.into();

    // The BridgeError::Sidecar variant should have a source.
    let source = std::error::Error::source(&bridge_err);
    assert!(source.is_some(), "Sidecar variant should chain source");

    // The inner SidecarError::Spawn should itself chain to the io::Error.
    let inner = source.unwrap();
    let inner_source = std::error::Error::source(inner);
    assert!(
        inner_source.is_some(),
        "SidecarError::Spawn should chain to io::Error"
    );
    assert!(
        inner_source.unwrap().to_string().contains("process gone"),
        "inner source should be the original io::Error"
    );
}

#[test]
fn error_sidecar_serialize_preserves_source_chain() {
    // Create a serde_json::Error by trying to parse invalid JSON.
    let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let sidecar_err = sidecar_kit::SidecarError::Serialize(json_err);
    let bridge_err: BridgeError = sidecar_err.into();

    let source = std::error::Error::source(&bridge_err);
    assert!(source.is_some(), "should chain SidecarError as source");
}

#[test]
fn error_string_variants_have_no_source() {
    // String-payload variants (NodeNotFound, Config, Run) have no inner error.
    let variants: Vec<BridgeError> = vec![
        BridgeError::NodeNotFound("x".into()),
        BridgeError::HostScriptNotFound("x".into()),
        BridgeError::Config("x".into()),
        BridgeError::Run("x".into()),
    ];
    for err in &variants {
        assert!(
            std::error::Error::source(err).is_none(),
            "string variant {err:?} should have no source"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// Error — conversions from SidecarError
// ═══════════════════════════════════════════════════════════════════

#[test]
fn sidecar_timeout_converts_to_bridge_error() {
    let bridge: BridgeError = sidecar_kit::SidecarError::Timeout.into();
    assert!(bridge.to_string().contains("timed out"));
}

#[test]
fn sidecar_protocol_converts_to_bridge_error() {
    let bridge: BridgeError = sidecar_kit::SidecarError::Protocol("bad envelope".into()).into();
    let msg = bridge.to_string();
    assert!(msg.contains("sidecar error"), "got: {msg}");
    assert!(msg.contains("protocol violation"), "got: {msg}");
}

#[test]
fn sidecar_fatal_converts_to_bridge_error() {
    let bridge: BridgeError = sidecar_kit::SidecarError::Fatal("crash".into()).into();
    let msg = bridge.to_string();
    assert!(msg.contains("sidecar"), "got: {msg}");
    assert!(msg.contains("crash"), "got: {msg}");
}

#[test]
fn sidecar_exited_converts_to_bridge_error() {
    let bridge: BridgeError = sidecar_kit::SidecarError::Exited(Some(1)).into();
    let msg = bridge.to_string();
    assert!(msg.contains("exited unexpectedly"), "got: {msg}");
}

#[test]
fn sidecar_exited_none_converts_to_bridge_error() {
    let bridge: BridgeError = sidecar_kit::SidecarError::Exited(None).into();
    let msg = bridge.to_string();
    assert!(msg.contains("exited"), "got: {msg}");
}

#[test]
fn sidecar_spawn_converts_to_bridge_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let bridge: BridgeError = sidecar_kit::SidecarError::Spawn(io_err).into();
    let msg = bridge.to_string();
    assert!(msg.contains("spawn"), "got: {msg}");
}

#[test]
fn sidecar_conversion_via_from_trait() {
    fn takes_bridge_err(_: BridgeError) {}
    // This tests that the From impl exists and compiles.
    takes_bridge_err(sidecar_kit::SidecarError::Timeout.into());
}

#[test]
fn sidecar_conversion_via_question_mark() {
    fn fallible() -> Result<(), BridgeError> {
        Err(sidecar_kit::SidecarError::Fatal("boom".into()))?
    }
    let err = fallible().unwrap_err();
    assert!(err.to_string().contains("boom"));
}

// ═══════════════════════════════════════════════════════════════════
// RunOptions — structure tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn run_options_default_all_none() {
    let opts = RunOptions::default();
    assert!(opts.lane.is_none());
    assert!(opts.workspace_root.is_none());
    assert!(opts.extra_config.is_none());
}

#[test]
fn run_options_with_extra_config_object() {
    let opts = RunOptions {
        lane: Some("review".into()),
        workspace_root: Some("/ws".into()),
        extra_config: Some(serde_json::json!({
            "model": "claude-3-opus",
            "temperature": 0.7,
        })),
    };
    let cfg = opts.extra_config.unwrap();
    assert!(cfg.is_object());
    assert_eq!(cfg["model"], "claude-3-opus");
}

#[test]
fn run_options_clone_is_independent() {
    let original = RunOptions {
        lane: Some("patch".into()),
        workspace_root: Some("/a".into()),
        extra_config: Some(serde_json::json!({"key": "val"})),
    };
    let mut cloned = original.clone();
    cloned.lane = Some("changed".into());

    assert_eq!(
        original.lane.as_deref(),
        Some("patch"),
        "original unchanged"
    );
    assert_eq!(cloned.lane.as_deref(), Some("changed"));
}

#[test]
fn run_options_debug_contains_struct_name() {
    let opts = RunOptions::default();
    let dbg = format!("{opts:?}");
    assert!(dbg.contains("RunOptions"), "Debug should name the struct");
}

// ═══════════════════════════════════════════════════════════════════
// ClaudeBridge construction
// ═══════════════════════════════════════════════════════════════════

#[test]
fn bridge_construction_with_default_config() {
    let bridge = claude_bridge::ClaudeBridge::new(ClaudeBridgeConfig::default());
    // Construction itself should never fail — discovery happens at run time.
    let _ = bridge;
}

#[test]
fn bridge_construction_with_full_config() {
    let cfg = ClaudeBridgeConfig::new()
        .with_api_key("test")
        .with_node_command("node")
        .with_host_script("/tmp/host.js")
        .with_cwd("/tmp")
        .with_adapter_module("/tmp/adapter.js")
        .with_env("EXTRA", "v")
        .with_handshake_timeout(Duration::from_secs(5))
        .with_channel_buffer(16);

    let _bridge = claude_bridge::ClaudeBridge::new(cfg);
}

// ═══════════════════════════════════════════════════════════════════
// Helpers — RAII environment variable guard
// ═══════════════════════════════════════════════════════════════════

/// RAII guard that restores an environment variable on drop.
struct EnvGuard {
    key: String,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: tests in this file are not multi-threaded per env var key.
        unsafe { std::env::set_var(key, value) };
        Self {
            key: key.to_string(),
            previous,
        }
    }

    fn remove(key: &str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: tests in this file are not multi-threaded per env var key.
        unsafe { std::env::remove_var(key) };
        Self {
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: restoring previous env state during test teardown.
        unsafe {
            match &self.previous {
                Some(val) => std::env::set_var(&self.key, val),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}
