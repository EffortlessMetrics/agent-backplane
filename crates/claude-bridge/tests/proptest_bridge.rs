// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for claude-bridge types.

use std::path::PathBuf;
use std::time::Duration;

use proptest::prelude::*;

use claude_bridge::config::ClaudeBridgeConfig;
use claude_bridge::error::BridgeError;
use claude_bridge::raw::RunOptions;

// ── Strategies ─────────────────────────────────────────────────────

fn arb_non_empty_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_./\\-]{1,64}"
}

fn arb_env_key() -> impl Strategy<Value = String> {
    "[A-Z_]{1,32}"
}

fn arb_env_value() -> impl Strategy<Value = String> {
    "[ -~]{0,128}" // printable ASCII
}

fn arb_duration() -> impl Strategy<Value = Duration> {
    (0u64..3600).prop_map(Duration::from_secs)
}

fn arb_channel_buffer() -> impl Strategy<Value = usize> {
    0usize..=4096
}

// ── Config builder properties ──────────────────────────────────────

proptest! {
    #[test]
    fn config_api_key_roundtrip(key in arb_non_empty_string()) {
        let cfg = ClaudeBridgeConfig::new().with_api_key(&key);
        prop_assert_eq!(cfg.env.get("ANTHROPIC_API_KEY").unwrap(), &key);
    }

    #[test]
    fn config_env_roundtrip(k in arb_env_key(), v in arb_env_value()) {
        let cfg = ClaudeBridgeConfig::new().with_env(&k, &v);
        prop_assert_eq!(cfg.env.get(&k).unwrap(), &v);
    }

    #[test]
    fn config_node_command_roundtrip(cmd in arb_non_empty_string()) {
        let cfg = ClaudeBridgeConfig::new().with_node_command(&cmd);
        prop_assert_eq!(cfg.node_command.as_deref(), Some(cmd.as_str()));
    }

    #[test]
    fn config_host_script_roundtrip(path in arb_non_empty_string()) {
        let cfg = ClaudeBridgeConfig::new().with_host_script(&path);
        prop_assert_eq!(cfg.host_script, Some(PathBuf::from(&path)));
    }

    #[test]
    fn config_cwd_roundtrip(path in arb_non_empty_string()) {
        let cfg = ClaudeBridgeConfig::new().with_cwd(&path);
        prop_assert_eq!(cfg.cwd, Some(PathBuf::from(&path)));
    }

    #[test]
    fn config_adapter_module_roundtrip(path in arb_non_empty_string()) {
        let cfg = ClaudeBridgeConfig::new().with_adapter_module(&path);
        prop_assert_eq!(cfg.adapter_module, Some(PathBuf::from(&path)));
    }

    #[test]
    fn config_handshake_timeout_roundtrip(d in arb_duration()) {
        let cfg = ClaudeBridgeConfig::new().with_handshake_timeout(d);
        prop_assert_eq!(cfg.handshake_timeout, d);
    }

    #[test]
    fn config_channel_buffer_roundtrip(n in arb_channel_buffer()) {
        let cfg = ClaudeBridgeConfig::new().with_channel_buffer(n);
        prop_assert_eq!(cfg.channel_buffer, n);
    }

    #[test]
    fn config_clone_equals_original(
        key in arb_non_empty_string(),
        cmd in arb_non_empty_string(),
        buf in arb_channel_buffer(),
        d in arb_duration(),
    ) {
        let cfg = ClaudeBridgeConfig::new()
            .with_api_key(&key)
            .with_node_command(&cmd)
            .with_channel_buffer(buf)
            .with_handshake_timeout(d);
        let cloned = cfg.clone();
        prop_assert_eq!(cloned.env, cfg.env);
        prop_assert_eq!(cloned.node_command, cfg.node_command);
        prop_assert_eq!(cloned.channel_buffer, cfg.channel_buffer);
        prop_assert_eq!(cloned.handshake_timeout, cfg.handshake_timeout);
    }

    #[test]
    fn config_multiple_env_entries(
        entries in proptest::collection::vec((arb_env_key(), arb_env_value()), 0..20)
    ) {
        let mut cfg = ClaudeBridgeConfig::new();
        for (k, v) in &entries {
            cfg = cfg.with_env(k, v);
        }
        // Each unique key should be present with the last value set
        let mut expected = std::collections::BTreeMap::new();
        for (k, v) in &entries {
            expected.insert(k.clone(), v.clone());
        }
        prop_assert_eq!(&cfg.env, &expected);
    }

    #[test]
    fn config_api_key_overwrite_last_wins(a in arb_non_empty_string(), b in arb_non_empty_string()) {
        let cfg = ClaudeBridgeConfig::new()
            .with_api_key(&a)
            .with_api_key(&b);
        prop_assert_eq!(cfg.env.get("ANTHROPIC_API_KEY").unwrap(), &b);
    }
}

// ── Error display properties ───────────────────────────────────────

proptest! {
    #[test]
    fn error_node_display_contains_message(msg in arb_non_empty_string()) {
        let err = BridgeError::NodeNotFound(msg.clone());
        let display = err.to_string();
        prop_assert!(display.contains(&msg));
        prop_assert!(display.contains("node.js not found"));
    }

    #[test]
    fn error_host_script_display_contains_message(msg in arb_non_empty_string()) {
        let err = BridgeError::HostScriptNotFound(msg.clone());
        let display = err.to_string();
        prop_assert!(display.contains(&msg));
        prop_assert!(display.contains("host script not found"));
    }

    #[test]
    fn error_config_display_contains_message(msg in arb_non_empty_string()) {
        let err = BridgeError::Config(msg.clone());
        let display = err.to_string();
        prop_assert!(display.contains(&msg));
        prop_assert!(display.contains("configuration error"));
    }

    #[test]
    fn error_run_display_contains_message(msg in arb_non_empty_string()) {
        let err = BridgeError::Run(msg.clone());
        let display = err.to_string();
        prop_assert!(display.contains(&msg));
        prop_assert!(display.contains("run error"));
    }

    #[test]
    fn error_display_never_empty(msg in arb_non_empty_string()) {
        let errors: Vec<BridgeError> = vec![
            BridgeError::NodeNotFound(msg.clone()),
            BridgeError::HostScriptNotFound(msg.clone()),
            BridgeError::Config(msg.clone()),
            BridgeError::Run(msg.clone()),
        ];
        for err in &errors {
            prop_assert!(!err.to_string().is_empty());
        }
    }
}

// ── RunOptions properties ──────────────────────────────────────────

proptest! {
    #[test]
    fn run_options_fields_stored_correctly(
        lane in proptest::option::of(arb_non_empty_string()),
        ws in proptest::option::of(arb_non_empty_string()),
    ) {
        let opts = RunOptions {
            lane: lane.clone(),
            workspace_root: ws.clone(),
            extra_config: None,
        };
        prop_assert_eq!(opts.lane, lane);
        prop_assert_eq!(opts.workspace_root, ws);
    }

    #[test]
    fn run_options_clone_equals_original(
        lane in proptest::option::of(arb_non_empty_string()),
        ws in proptest::option::of(arb_non_empty_string()),
    ) {
        let opts = RunOptions {
            lane: lane.clone(),
            workspace_root: ws.clone(),
            extra_config: Some(serde_json::json!({"key": "value"})),
        };
        let cloned = opts.clone();
        prop_assert_eq!(cloned.lane, opts.lane);
        prop_assert_eq!(cloned.workspace_root, opts.workspace_root);
        prop_assert_eq!(cloned.extra_config, opts.extra_config);
    }
}
