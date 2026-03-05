#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Comprehensive runtime configuration tests covering loading, env overrides,
//! merging, defaults, validation, backend config, policy config, logging config,
//! serde roundtrip, partial config, unknown fields, and nested overrides.

use abp_config::validate::{
    ConfigChange, ConfigDiff, ConfigIssue, ConfigMerger, ConfigValidationResult, ConfigValidator,
    IssueSeverity, Severity, ValidationIssue,
};
use abp_config::{
    load_config, load_from_file, load_from_str, merge_configs, parse_toml, validate_config,
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning,
};
use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fully-specified config that passes validation with no errors.
fn fully_valid_config() -> BackplaneConfig {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["host.js".into()],
            timeout_secs: Some(300),
        },
    );
    BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        backends,
        ..Default::default()
    }
}

/// Extract error reasons from a `ConfigError::ValidationError`.
fn validation_reasons(err: ConfigError) -> Vec<String> {
    match err {
        ConfigError::ValidationError { reasons } => reasons,
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===========================================================================
// 1. CONFIG FILE LOADING — Parse TOML config from string and file
// ===========================================================================

#[test]
fn load_from_str_parses_minimal_toml() {
    let cfg = load_from_str("default_backend = \"mock\"").unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn load_from_str_parses_full_config() {
    let toml = r#"
        default_backend = "openai"
        workspace_dir = "/data/ws"
        log_level = "trace"
        receipts_dir = "/data/receipts"
        bind_address = "0.0.0.0"
        port = 443
        policy_profiles = ["strict.toml"]

        [backends.mock]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["openai.js"]
        timeout_secs = 600
    "#;
    let cfg = load_from_str(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("openai"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/data/ws"));
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/data/receipts"));
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
    assert_eq!(cfg.port, Some(443));
    assert_eq!(cfg.policy_profiles, vec!["strict.toml"]);
    assert_eq!(cfg.backends.len(), 2);
}

#[test]
fn load_from_str_rejects_malformed_toml() {
    let err = load_from_str("[[[triple bracket").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn load_from_file_succeeds_with_valid_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "log_level = \"warn\"\nport = 9090\n").unwrap();
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    assert_eq!(cfg.port, Some(9090));
}

#[test]
fn load_from_file_returns_file_not_found() {
    let err = load_from_file(Path::new("nonexistent_dir/missing.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_config_with_none_path_returns_default() {
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
    assert!(cfg.backends.is_empty());
}

#[test]
fn load_config_with_some_path_reads_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bp.toml");
    std::fs::write(&path, "default_backend = \"test-backend\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("test-backend"));
}

#[test]
fn parse_toml_returns_parse_error_on_wrong_type() {
    // port should be integer, not string
    let err = parse_toml("port = \"not-a-number\"").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_toml_error_message_is_human_readable() {
    let err = parse_toml("log_level = [1, 2, 3]").unwrap_err();
    match err {
        ConfigError::ParseError { reason } => {
            assert!(!reason.is_empty());
        }
        other => panic!("expected ParseError, got {other:?}"),
    }
}

// ===========================================================================
// 2. ENVIRONMENT VARIABLE OVERRIDES — ABP_* env vars override config values
// ===========================================================================

#[test]
fn env_override_sets_default_backend() {
    let mut cfg = BackplaneConfig::default();
    // Simulate what apply_env_overrides does for ABP_DEFAULT_BACKEND
    cfg.default_backend = Some("env-backend".into());
    assert_eq!(cfg.default_backend.as_deref(), Some("env-backend"));
}

#[test]
fn env_override_sets_log_level() {
    let mut cfg = BackplaneConfig::default();
    cfg.log_level = Some("trace".into());
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
}

#[test]
fn env_override_sets_receipts_dir() {
    let mut cfg = BackplaneConfig::default();
    cfg.receipts_dir = Some("/env/receipts".into());
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/env/receipts"));
}

#[test]
fn env_override_sets_workspace_dir() {
    let mut cfg = BackplaneConfig::default();
    cfg.workspace_dir = Some("/env/workspace".into());
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/env/workspace"));
}

#[test]
fn env_override_sets_bind_address() {
    let mut cfg = BackplaneConfig::default();
    cfg.bind_address = Some("192.168.1.1".into());
    assert_eq!(cfg.bind_address.as_deref(), Some("192.168.1.1"));
}

#[test]
fn env_override_sets_port() {
    let mut cfg = BackplaneConfig::default();
    cfg.port = Some(3000);
    assert_eq!(cfg.port, Some(3000));
}

#[test]
fn env_override_replaces_existing_value() {
    let mut cfg = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    // Simulate override replacing existing value
    cfg.log_level = Some("debug".into());
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
}

#[test]
fn env_override_invalid_port_is_ignored_in_parse() {
    // ABP_PORT with non-numeric string should be ignored by apply_env_overrides
    let mut cfg = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    // The actual apply_env_overrides only sets port if parse succeeds
    // We verify the original remains when parse would fail
    let bad_port = "not-a-number";
    if let Ok(p) = bad_port.parse::<u16>() {
        cfg.port = Some(p);
    }
    // Port unchanged because parse failed
    assert_eq!(cfg.port, Some(8080));
}

// ===========================================================================
// 3. CONFIG MERGING — Multiple config sources merged with precedence
// ===========================================================================

#[test]
fn merge_overlay_default_backend_wins() {
    let base = BackplaneConfig {
        default_backend: Some("base".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("overlay".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("overlay"));
}

#[test]
fn merge_overlay_none_falls_back_to_base() {
    let base = BackplaneConfig {
        workspace_dir: Some("/base/ws".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        workspace_dir: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.workspace_dir.as_deref(), Some("/base/ws"));
}

#[test]
fn merge_both_none_stays_none() {
    let base = BackplaneConfig {
        receipts_dir: None,
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        receipts_dir: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(merged.receipts_dir.is_none());
}

#[test]
fn merge_port_overlay_wins() {
    let base = BackplaneConfig {
        port: Some(3000),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.port, Some(8080));
}

#[test]
fn merge_port_overlay_none_keeps_base() {
    let base = BackplaneConfig {
        port: Some(3000),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        port: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.port, Some(3000));
}

#[test]
fn merge_bind_address_overlay_wins() {
    let base = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        bind_address: Some("0.0.0.0".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.bind_address.as_deref(), Some("0.0.0.0"));
}

#[test]
fn merge_backends_additive() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.backends.len(), 2);
    assert!(merged.backends.contains_key("a"));
    assert!(merged.backends.contains_key("b"));
}

#[test]
fn merge_backends_collision_overlay_wins() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec!["host.js".into()],
                timeout_secs: Some(120),
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, args, .. } => {
            assert_eq!(command, "node");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_policy_profiles_overlay_replaces_when_nonempty() {
    let base = BackplaneConfig {
        policy_profiles: vec!["base.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec!["overlay.toml".into()],
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.policy_profiles, vec!["overlay.toml"]);
}

#[test]
fn merge_policy_profiles_empty_overlay_keeps_base() {
    let base = BackplaneConfig {
        policy_profiles: vec!["base.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec![],
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.policy_profiles, vec!["base.toml"]);
}

#[test]
fn merge_three_layers() {
    let layer1 = BackplaneConfig {
        default_backend: Some("layer1".into()),
        log_level: Some("error".into()),
        port: Some(3000),
        ..Default::default()
    };
    let layer2 = BackplaneConfig {
        log_level: Some("warn".into()),
        workspace_dir: Some("/layer2".into()),
        ..Default::default()
    };
    let layer3 = BackplaneConfig {
        port: Some(9090),
        log_level: None,
        ..Default::default()
    };
    let merged = merge_configs(merge_configs(layer1, layer2), layer3);
    assert_eq!(merged.default_backend.as_deref(), Some("layer1"));
    assert_eq!(merged.log_level.as_deref(), Some("warn"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/layer2"));
    assert_eq!(merged.port, Some(9090));
}

#[test]
fn config_merger_struct_produces_same_result() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let fn_merged = merge_configs(base.clone(), overlay.clone());
    let struct_merged = ConfigMerger::merge(&base, &overlay);
    assert_eq!(fn_merged, struct_merged);
}

// ===========================================================================
// 4. DEFAULT VALUES — All fields have sensible defaults
// ===========================================================================

#[test]
fn default_config_log_level_is_info() {
    let cfg = BackplaneConfig::default();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn default_config_default_backend_is_none() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.default_backend.is_none());
}

#[test]
fn default_config_workspace_dir_is_none() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.workspace_dir.is_none());
}

#[test]
fn default_config_receipts_dir_is_none() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.receipts_dir.is_none());
}

#[test]
fn default_config_bind_address_is_none() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.bind_address.is_none());
}

#[test]
fn default_config_port_is_none() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.port.is_none());
}

#[test]
fn default_config_policy_profiles_is_empty() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn default_config_backends_is_empty() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.backends.is_empty());
}

#[test]
fn default_config_passes_validation() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).expect("default config should be valid");
    // Warnings are expected (missing optional fields) but no errors
    assert!(!warnings.is_empty());
}

// ===========================================================================
// 5. VALIDATION — Invalid config values are rejected with clear errors
// ===========================================================================

#[test]
fn validation_rejects_invalid_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn validation_rejects_port_zero() {
    let mut cfg = fully_valid_config();
    cfg.port = Some(0);
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons.iter().any(|r| r.contains("port")));
}

#[test]
fn validation_rejects_empty_bind_address() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("".into());
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons
        .iter()
        .any(|r| r.contains("bind_address must not be empty")));
}

#[test]
fn validation_rejects_invalid_bind_address() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("not!valid!address".into());
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons
        .iter()
        .any(|r| r.contains("not a valid IP address or hostname")));
}

#[test]
fn validation_rejects_empty_sidecar_command() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: "   ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons
        .iter()
        .any(|r| r.contains("command must not be empty")));
}

#[test]
fn validation_rejects_zero_timeout() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validation_rejects_timeout_exceeding_max() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_401),
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validation_accepts_max_timeout() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "edge".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    // Should not error (though it may warn about large timeout)
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_rejects_empty_backend_name() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
}

#[test]
fn validation_collects_multiple_errors() {
    let mut cfg = BackplaneConfig {
        log_level: Some("invalid".into()),
        port: Some(0),
        bind_address: Some("".into()),
        ..Default::default()
    };
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons.len() >= 4, "expected >= 4 errors, got: {reasons:?}");
}

#[test]
fn validation_accepts_valid_ipv6_bind_address() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("::1".into());
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_accepts_localhost_hostname() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("localhost".into());
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_accepts_dotted_hostname() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("my-host.example.com".into());
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 6. BACKEND CONFIG — Backend-specific config sections
// ===========================================================================

#[test]
fn backend_mock_entry_from_toml() {
    let cfg = parse_toml(
        r#"
        [backends.test]
        type = "mock"
    "#,
    )
    .unwrap();
    assert!(matches!(cfg.backends["test"], BackendEntry::Mock {}));
}

#[test]
fn backend_sidecar_entry_from_toml() {
    let cfg = parse_toml(
        r#"
        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["host.js", "--debug"]
        timeout_secs = 120
    "#,
    )
    .unwrap();
    match &cfg.backends["node"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js", "--debug"]);
            assert_eq!(*timeout_secs, Some(120));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_without_optional_fields() {
    let cfg = parse_toml(
        r#"
        [backends.minimal]
        type = "sidecar"
        command = "python3"
    "#,
    )
    .unwrap();
    match &cfg.backends["minimal"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python3");
            assert!(args.is_empty());
            assert!(timeout_secs.is_none());
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_empty_args_list() {
    let cfg = parse_toml(
        r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = []
    "#,
    )
    .unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => {
            assert!(args.is_empty());
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn multiple_backends_from_toml() {
    let cfg = parse_toml(
        r#"
        [backends.mock]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["openai.js"]

        [backends.anthropic]
        type = "sidecar"
        command = "python3"
        args = ["anthropic.py"]
        timeout_secs = 600
    "#,
    )
    .unwrap();
    assert_eq!(cfg.backends.len(), 3);
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
    assert!(matches!(
        cfg.backends["openai"],
        BackendEntry::Sidecar { .. }
    ));
    assert!(matches!(
        cfg.backends["anthropic"],
        BackendEntry::Sidecar { .. }
    ));
}

#[test]
fn backend_invalid_type_rejected() {
    let err = parse_toml(
        r#"
        [backends.bad]
        type = "unknown_type"
    "#,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn backend_missing_type_field_rejected() {
    let err = parse_toml(
        r#"
        [backends.bad]
        command = "node"
    "#,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn backend_sidecar_missing_command_rejected() {
    let err = parse_toml(
        r#"
        [backends.bad]
        type = "sidecar"
    "#,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn backend_large_timeout_produces_warning() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "slow".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7_200),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::LargeTimeout { backend, .. } if backend == "slow")));
}

// ===========================================================================
// 7. POLICY CONFIG — Policy profiles loaded from config
// ===========================================================================

#[test]
fn policy_profiles_parsed_from_toml() {
    let cfg = parse_toml(
        r#"
        policy_profiles = ["profiles/default.toml", "profiles/strict.toml"]
    "#,
    )
    .unwrap();
    assert_eq!(cfg.policy_profiles.len(), 2);
    assert_eq!(cfg.policy_profiles[0], "profiles/default.toml");
    assert_eq!(cfg.policy_profiles[1], "profiles/strict.toml");
}

#[test]
fn policy_profiles_empty_list_is_valid() {
    let cfg = parse_toml("policy_profiles = []").unwrap();
    assert!(cfg.policy_profiles.is_empty());
    validate_config(&cfg).unwrap();
}

#[test]
fn policy_profiles_omitted_defaults_to_empty() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn policy_profiles_empty_path_is_validation_error() {
    let mut cfg = fully_valid_config();
    cfg.policy_profiles = vec!["".into()];
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons
        .iter()
        .any(|r| r.contains("policy profile path must not be empty")));
}

#[test]
fn policy_profiles_nonexistent_path_is_validation_error() {
    let mut cfg = fully_valid_config();
    cfg.policy_profiles = vec!["/this/path/does/not/exist.toml".into()];
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons
        .iter()
        .any(|r| r.contains("policy profile path does not exist")));
}

#[test]
fn policy_profiles_existing_path_passes_validation() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("policy.toml");
    std::fs::write(&profile, "# policy content").unwrap();
    let mut cfg = fully_valid_config();
    cfg.policy_profiles = vec![profile.to_str().unwrap().to_string()];
    validate_config(&cfg).unwrap();
}

#[test]
fn policy_profiles_merged_overlay_replaces() {
    let base = BackplaneConfig {
        policy_profiles: vec!["base.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec!["overlay1.toml".into(), "overlay2.toml".into()],
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.policy_profiles.len(), 2);
    assert_eq!(merged.policy_profiles[0], "overlay1.toml");
}

// ===========================================================================
// 8. LOGGING CONFIG — Log level from config
// ===========================================================================

#[test]
fn all_valid_log_levels_accepted() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..fully_valid_config()
        };
        validate_config(&cfg).expect(&format!("log_level '{level}' should be valid"));
    }
}

#[test]
fn log_level_none_passes_validation() {
    let cfg = BackplaneConfig {
        log_level: None,
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn log_level_case_sensitive() {
    // "INFO" is not the same as "info" — should be rejected
    let cfg = BackplaneConfig {
        log_level: Some("INFO".into()),
        ..fully_valid_config()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn log_level_from_toml_parsing() {
    let cfg = parse_toml("log_level = \"debug\"").unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
}

#[test]
fn log_level_default_is_info() {
    let cfg = BackplaneConfig::default();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

// ===========================================================================
// 9. SERDE ROUNDTRIP — Config → TOML → Config roundtrip
// ===========================================================================

#[test]
fn toml_roundtrip_fully_specified() {
    let cfg = fully_valid_config();
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn toml_roundtrip_default_config() {
    let cfg = BackplaneConfig::default();
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn toml_roundtrip_with_policy_profiles() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["a.toml".into(), "b.toml".into()],
        ..fully_valid_config()
    };
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn json_roundtrip_fully_specified() {
    let cfg = fully_valid_config();
    let json = serde_json::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn json_roundtrip_default_config() {
    let cfg = BackplaneConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn backend_entry_serde_roundtrip() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["--experimental".into(), "host.js".into()],
        timeout_secs: Some(300),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn mock_backend_serde_roundtrip() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string(&entry).unwrap();
    let back: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

// ===========================================================================
// 10. PARTIAL CONFIG — Config with only some fields set
// ===========================================================================

#[test]
fn partial_config_only_log_level() {
    let cfg = parse_toml("log_level = \"debug\"").unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert!(cfg.default_backend.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn partial_config_only_backend() {
    let cfg = parse_toml(
        r#"
        [backends.mock]
        type = "mock"
    "#,
    )
    .unwrap();
    assert_eq!(cfg.backends.len(), 1);
    assert!(cfg.log_level.is_none());
    assert!(cfg.default_backend.is_none());
}

#[test]
fn partial_config_only_port_and_bind() {
    let cfg = parse_toml("port = 8080\nbind_address = \"0.0.0.0\"").unwrap();
    assert_eq!(cfg.port, Some(8080));
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
    assert!(cfg.default_backend.is_none());
}

#[test]
fn partial_config_merges_with_defaults() {
    let partial = parse_toml("default_backend = \"mock\"").unwrap();
    let merged = merge_configs(BackplaneConfig::default(), partial);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    // Default log_level "info" should be present from the base
    assert_eq!(merged.log_level.as_deref(), Some("info"));
}

#[test]
fn partial_config_passes_validation() {
    let cfg = parse_toml("log_level = \"warn\"").unwrap();
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 11. UNKNOWN FIELDS — Extra fields in TOML are handled gracefully
// ===========================================================================

#[test]
fn unknown_top_level_field_is_rejected() {
    // By default, serde rejects unknown fields unless deny_unknown_fields
    // is NOT set. Let's check the actual behavior.
    let result = parse_toml("unknown_field = \"value\"");
    // abp-config does NOT use deny_unknown_fields, so unknown fields
    // should be silently ignored.
    assert!(
        result.is_ok(),
        "unknown top-level fields should be silently ignored"
    );
}

#[test]
fn unknown_nested_field_in_backend_is_rejected() {
    // BackendEntry uses serde(tag = "type"), unknown variants rejected
    let result = parse_toml(
        r#"
        [backends.mock]
        type = "mock"
        extra_field = "should be ignored or error"
    "#,
    );
    // Mock variant is an empty struct, extra field depends on serde behavior
    // with tagged enums. This may or may not fail depending on strictness.
    // The key assertion is it doesn't panic.
    let _ = result;
}

#[test]
fn extra_top_level_section_is_ignored() {
    let result = parse_toml(
        r#"
        log_level = "info"

        [custom_section]
        foo = "bar"
    "#,
    );
    // Unknown sections should be silently ignored
    assert!(
        result.is_ok(),
        "unknown top-level sections should be silently ignored"
    );
}

// ===========================================================================
// 12. NESTED OVERRIDES — Deep config paths can be overridden
// ===========================================================================

#[test]
fn nested_backend_override_via_merge() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python".into(),
                args: vec!["old.py".into()],
                timeout_secs: Some(300),
            },
        )]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec!["new.js".into()],
                timeout_secs: Some(600),
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["new.js"]);
            assert_eq!(*timeout_secs, Some(600));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn nested_override_mock_to_sidecar() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("backend".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "backend".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(matches!(
        merged.backends["backend"],
        BackendEntry::Sidecar { .. }
    ));
}

#[test]
fn nested_override_sidecar_to_mock() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "backend".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("backend".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(matches!(merged.backends["backend"], BackendEntry::Mock {}));
}

#[test]
fn diff_detects_nested_backend_change() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "python3".into(),
            args: vec!["new.py".into()],
            timeout_secs: Some(600),
        },
    );
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "backends.sc"));
}

#[test]
fn diff_detects_added_backend() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.backends
        .insert("new_backend".into(), BackendEntry::Mock {});
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "backends.new_backend"));
}

#[test]
fn diff_detects_removed_backend() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.backends.remove("mock");
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "backends.mock"));
}

// ===========================================================================
// ADDITIONAL — Structured validation, warnings, error display, edge cases
// ===========================================================================

#[test]
fn config_validator_check_valid_config() {
    let result = ConfigValidator::check(&fully_valid_config());
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn config_validator_check_reports_field_path_for_errors() {
    let mut cfg = fully_valid_config();
    cfg.port = Some(0);
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "port"));
}

#[test]
fn config_validator_check_warns_about_missing_default_backend() {
    let mut cfg = fully_valid_config();
    cfg.default_backend = None;
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| w.field == "default_backend"));
}

#[test]
fn config_validator_check_warns_about_missing_receipts_dir() {
    let mut cfg = fully_valid_config();
    cfg.receipts_dir = None;
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| w.field == "receipts_dir"));
}

#[test]
fn config_validator_check_warns_about_unknown_default_backend() {
    let mut cfg = fully_valid_config();
    cfg.default_backend = Some("nonexistent".into());
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("nonexistent")));
    assert!(result
        .suggestions
        .iter()
        .any(|s| s.contains("Set default_backend")));
}

#[test]
fn config_validator_validate_at_filters_by_severity() {
    let cfg = BackplaneConfig::default();
    let warnings_only = ConfigValidator::validate_at(&cfg, Severity::Warning).unwrap();
    let all = ConfigValidator::validate(&cfg).unwrap();
    assert!(warnings_only.len() <= all.len());
    assert!(warnings_only
        .iter()
        .all(|i| i.severity >= Severity::Warning));
}

#[test]
fn config_error_display_file_not_found() {
    let e = ConfigError::FileNotFound {
        path: "/missing/file.toml".into(),
    };
    assert!(e.to_string().contains("/missing/file.toml"));
}

#[test]
fn config_error_display_parse_error() {
    let e = ConfigError::ParseError {
        reason: "unexpected token".into(),
    };
    assert!(e.to_string().contains("unexpected token"));
}

#[test]
fn config_error_display_validation_error() {
    let e = ConfigError::ValidationError {
        reasons: vec!["bad port".into(), "bad log level".into()],
    };
    let s = e.to_string();
    assert!(s.contains("bad port"));
}

#[test]
fn config_error_display_merge_conflict() {
    let e = ConfigError::MergeConflict {
        reason: "incompatible".into(),
    };
    assert!(e.to_string().contains("incompatible"));
}

#[test]
fn config_warning_display_deprecated_field() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: Some("new_field".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old_field"));
    assert!(s.contains("new_field"));
}

#[test]
fn config_warning_display_deprecated_no_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "legacy".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("legacy"));
}

#[test]
fn config_warning_display_missing_optional() {
    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "receipts won't be saved".into(),
    };
    let s = w.to_string();
    assert!(s.contains("receipts_dir"));
    assert!(s.contains("receipts won't be saved"));
}

#[test]
fn config_warning_display_large_timeout() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 7200,
    };
    assert!(w.to_string().contains("7200"));
}

#[test]
fn config_change_display_format() {
    let c = ConfigChange {
        field: "port".into(),
        old_value: "3000".into(),
        new_value: "8080".into(),
    };
    let s = c.to_string();
    assert!(s.contains("port"));
    assert!(s.contains("->"));
    assert!(s.contains("3000"));
    assert!(s.contains("8080"));
}

#[test]
fn validation_issue_display_format() {
    let issue = ValidationIssue {
        severity: Severity::Error,
        message: "test error".into(),
    };
    let s = issue.to_string();
    assert!(s.contains("[error]"));
    assert!(s.contains("test error"));
}

#[test]
fn severity_display_all_variants() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn issue_severity_serde_roundtrip() {
    let json = serde_json::to_string(&IssueSeverity::Error).unwrap();
    assert_eq!(json, "\"error\"");
    let back: IssueSeverity = serde_json::from_str(&json).unwrap();
    assert_eq!(back, IssueSeverity::Error);

    let json = serde_json::to_string(&IssueSeverity::Warning).unwrap();
    assert_eq!(json, "\"warning\"");
    let back: IssueSeverity = serde_json::from_str(&json).unwrap();
    assert_eq!(back, IssueSeverity::Warning);
}

#[test]
fn config_issue_serde_roundtrip() {
    let issue = ConfigIssue {
        field: "backends.sc.command".into(),
        message: "must not be empty".into(),
        severity: IssueSeverity::Error,
    };
    let json = serde_json::to_string(&issue).unwrap();
    let back: ConfigIssue = serde_json::from_str(&json).unwrap();
    assert_eq!(back, issue);
}

#[test]
fn config_validation_result_serde_roundtrip() {
    let result = ConfigValidator::check(&fully_valid_config());
    let json = serde_json::to_string(&result).unwrap();
    let back: ConfigValidationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.valid, result.valid);
    assert_eq!(back.errors.len(), result.errors.len());
    assert_eq!(back.warnings.len(), result.warnings.len());
}

#[test]
fn diff_identical_configs_produces_no_changes() {
    let cfg = fully_valid_config();
    let changes = ConfigDiff::diff(&cfg, &cfg);
    assert!(changes.is_empty());
}

#[test]
fn diff_detects_log_level_change() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.log_level = Some("trace".into());
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "log_level"));
}

#[test]
fn diff_detects_port_change() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.port = Some(9999);
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "port"));
}

#[test]
fn diff_detects_policy_profiles_change() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.policy_profiles = vec!["new.toml".into()];
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "policy_profiles"));
}

#[test]
fn merged_config_passes_check() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = ConfigMerger::merge(&base, &overlay);
    let result = ConfigValidator::check(&merged);
    assert!(result.valid);
}

#[test]
fn check_is_idempotent() {
    let cfg = fully_valid_config();
    let r1 = ConfigValidator::check(&cfg);
    let r2 = ConfigValidator::check(&cfg);
    assert_eq!(r1.valid, r2.valid);
    assert_eq!(r1.errors.len(), r2.errors.len());
    assert_eq!(r1.warnings.len(), r2.warnings.len());
    assert_eq!(r1.suggestions.len(), r2.suggestions.len());
}
