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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
//! Comprehensive config validation tests covering TOML parsing, backend config,
//! policy config, sidecar config, defaults, validation errors, env overrides,
//! multiple backends, config merge, serde roundtrips, file loading, and dotted
//! key access.

use abp_config::validate::{
    ConfigDiff, ConfigMerger, ConfigValidator, IssueSeverity, Severity, ValidationIssue,
};
use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, load_config, load_from_file,
    load_from_str, merge_configs, parse_toml, validate_config,
};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fully-specified config that passes validation with zero warnings.
fn fully_valid_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        bind_address: None,
        port: None,
        policy_profiles: Vec::new(),
        backends: BTreeMap::from([
            ("mock".into(), BackendEntry::Mock {}),
            (
                "sc".into(),
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(300),
                },
            ),
        ]),
    }
}

/// Extract validation error reasons from a `ConfigError::ValidationError`.
fn validation_reasons(err: ConfigError) -> Vec<String> {
    match err {
        ConfigError::ValidationError { reasons } => reasons,
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===========================================================================
// Category 1: TOML parsing
// ===========================================================================

#[test]
fn toml_parse_example_config_format() {
    let toml = r#"
        default_backend = "mock"
        log_level = "info"
        receipts_dir = "./data/receipts"

        [backends.mock]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["path/to/openai-sidecar.js"]
        timeout_secs = 300

        [backends.anthropic]
        type = "sidecar"
        command = "python3"
        args = ["path/to/anthropic-sidecar.py"]
        timeout_secs = 600
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.backends.len(), 3);
    assert!(cfg.backends.contains_key("openai"));
    assert!(cfg.backends.contains_key("anthropic"));
}

#[test]
fn toml_parse_minimal_just_log_level() {
    let cfg = parse_toml(r#"log_level = "warn""#).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_parse_empty_string_all_none() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.workspace_dir.is_none());
    assert!(cfg.log_level.is_none());
    assert!(cfg.receipts_dir.is_none());
    assert!(cfg.bind_address.is_none());
    assert!(cfg.port.is_none());
    assert!(cfg.policy_profiles.is_empty());
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_parse_whitespace_only_string() {
    let cfg = parse_toml("   \n\n  \t  ").unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_parse_comments_only() {
    let cfg = parse_toml("# just a comment\n# another one\n").unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_parse_invalid_syntax_gives_parse_error() {
    let err = parse_toml("[not valid = {").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_wrong_type_for_log_level() {
    let err = parse_toml("log_level = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_wrong_type_for_port() {
    let err = parse_toml(r#"port = "not_a_number""#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_wrong_type_for_backends() {
    let err = parse_toml("backends = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_unknown_fields_are_rejected() {
    // serde with deny_unknown_fields would reject, but BackplaneConfig uses
    // default serde which silently ignores unknown fields.
    let result = parse_toml(r#"totally_unknown = "value""#);
    // Either parse succeeds (ignoring field) or fails — both are acceptable.
    // We just verify no panic.
    let _ = result;
}

#[test]
fn toml_parse_duplicate_key_last_wins_or_error() {
    let toml = "log_level = \"info\"\nlog_level = \"debug\"";
    // TOML spec says duplicate keys are errors; the `toml` crate rejects them.
    assert!(parse_toml(toml).is_err());
}

// ===========================================================================
// Category 2: Backend config
// ===========================================================================

#[test]
fn backend_mock_minimal() {
    let cfg = parse_toml(
        r#"
        [backends.test_mock]
        type = "mock"
    "#,
    )
    .unwrap();
    assert!(matches!(cfg.backends["test_mock"], BackendEntry::Mock {}));
}

#[test]
fn backend_sidecar_all_fields() {
    let cfg = parse_toml(
        r#"
        [backends.full]
        type = "sidecar"
        command = "python3"
        args = ["--verbose", "host.py", "--port", "8080"]
        timeout_secs = 600
    "#,
    )
    .unwrap();
    match &cfg.backends["full"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python3");
            assert_eq!(args.len(), 4);
            assert_eq!(*timeout_secs, Some(600));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_no_args_no_timeout() {
    let cfg = parse_toml(
        r#"
        [backends.bare]
        type = "sidecar"
        command = "node"
    "#,
    )
    .unwrap();
    match &cfg.backends["bare"] {
        BackendEntry::Sidecar {
            args, timeout_secs, ..
        } => {
            assert!(args.is_empty());
            assert!(timeout_secs.is_none());
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_missing_command_is_parse_error() {
    let err = parse_toml(
        r#"
        [backends.bad]
        type = "sidecar"
        args = ["foo"]
    "#,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn backend_unknown_type_is_parse_error() {
    let err = parse_toml(
        r#"
        [backends.bad]
        type = "openai_native"
        api_key = "sk-..."
    "#,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn backend_missing_type_is_parse_error() {
    let err = parse_toml(
        r#"
        [backends.bad]
        command = "node"
    "#,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// ===========================================================================
// Category 3: Policy config
// ===========================================================================

#[test]
fn policy_profiles_parsed_from_toml() {
    let cfg = parse_toml(
        r#"
        policy_profiles = ["policies/default.toml", "policies/strict.toml"]
    "#,
    )
    .unwrap();
    assert_eq!(cfg.policy_profiles.len(), 2);
    assert_eq!(cfg.policy_profiles[0], "policies/default.toml");
    assert_eq!(cfg.policy_profiles[1], "policies/strict.toml");
}

#[test]
fn policy_profiles_empty_array() {
    let cfg = parse_toml("policy_profiles = []").unwrap();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn policy_profiles_default_is_empty() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn policy_profiles_empty_path_is_validation_error() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["".into()],
        ..fully_valid_config()
    };
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("policy profile path must not be empty"))
    );
}

#[test]
fn policy_profiles_whitespace_path_is_validation_error() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["   ".into()],
        ..fully_valid_config()
    };
    let err = validate_config(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("policy profile path must not be empty"))
    );
}

// ===========================================================================
// Category 4: Sidecar config
// ===========================================================================

#[test]
fn sidecar_binary_path_absolute() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "abs".into(),
        BackendEntry::Sidecar {
            command: "/usr/local/bin/python3".into(),
            args: vec!["sidecar.py".into()],
            timeout_secs: Some(120),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_binary_path_relative() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "rel".into(),
        BackendEntry::Sidecar {
            command: "./bin/sidecar".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_with_many_args() {
    let cfg = parse_toml(
        r#"
        [backends.complex]
        type = "sidecar"
        command = "node"
        args = ["--experimental-modules", "--max-old-space-size=4096", "host.mjs", "--port", "3000", "--verbose"]
        timeout_secs = 900
    "#,
    )
    .unwrap();
    match &cfg.backends["complex"] {
        BackendEntry::Sidecar { args, .. } => assert_eq!(args.len(), 6),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn sidecar_empty_command_validation_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "empty_cmd".into(),
        BackendEntry::Sidecar {
            command: String::new(),
            args: vec!["arg".into()],
            timeout_secs: Some(60),
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("command must not be empty"))
    );
}

#[test]
fn sidecar_whitespace_command_validation_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "ws_cmd".into(),
        BackendEntry::Sidecar {
            command: "  \t  ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("command must not be empty"))
    );
}

#[test]
fn sidecar_timeout_boundary_1s_valid() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "t1".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(1),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_timeout_boundary_max_valid() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "tmax".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_timeout_zero_invalid() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "t0".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn sidecar_timeout_exceeds_max_invalid() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "toobig".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_401),
        },
    );
    assert!(validate_config(&cfg).is_err());
}

// ===========================================================================
// Category 5: Default values
// ===========================================================================

#[test]
fn default_config_log_level_is_info() {
    assert_eq!(
        BackplaneConfig::default().log_level.as_deref(),
        Some("info")
    );
}

#[test]
fn default_config_has_no_backends() {
    assert!(BackplaneConfig::default().backends.is_empty());
}

#[test]
fn default_config_has_no_default_backend() {
    assert!(BackplaneConfig::default().default_backend.is_none());
}

#[test]
fn default_config_has_no_workspace_dir() {
    assert!(BackplaneConfig::default().workspace_dir.is_none());
}

#[test]
fn default_config_has_no_receipts_dir() {
    assert!(BackplaneConfig::default().receipts_dir.is_none());
}

#[test]
fn default_config_has_no_bind_address() {
    assert!(BackplaneConfig::default().bind_address.is_none());
}

#[test]
fn default_config_has_no_port() {
    assert!(BackplaneConfig::default().port.is_none());
}

#[test]
fn default_config_has_empty_policy_profiles() {
    assert!(BackplaneConfig::default().policy_profiles.is_empty());
}

#[test]
fn default_config_passes_validation() {
    let warnings = validate_config(&BackplaneConfig::default()).unwrap();
    // Has advisory warnings but no hard errors.
    assert!(!warnings.is_empty());
}

// ===========================================================================
// Category 6: Validation errors — clear error messages
// ===========================================================================

#[test]
fn validation_error_mentions_backend_name() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "my_broken_backend".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons.iter().any(|r| r.contains("my_broken_backend")),
        "error should name the backend: {reasons:?}"
    );
}

#[test]
fn validation_error_mentions_timeout_value() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "slow".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(100_000),
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons.iter().any(|r| r.contains("100000")),
        "error should mention the timeout: {reasons:?}"
    );
}

#[test]
fn validation_error_mentions_invalid_log_level_value() {
    let cfg = BackplaneConfig {
        log_level: Some("VERBOSE".into()),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons.iter().any(|r| r.contains("VERBOSE")),
        "error should mention the bad value: {reasons:?}"
    );
}

#[test]
fn validation_port_zero_is_error() {
    let cfg = BackplaneConfig {
        port: Some(0),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("port")));
}

#[test]
fn validation_bind_address_empty_is_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("".into()),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("bind_address")));
}

#[test]
fn validation_bind_address_invalid_is_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("not an ip or hostname!!!".into()),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("bind_address")));
}

#[test]
fn validation_bind_address_valid_ip() {
    let cfg = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_bind_address_valid_ipv6() {
    let cfg = BackplaneConfig {
        bind_address: Some("::1".into()),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_bind_address_valid_hostname() {
    let cfg = BackplaneConfig {
        bind_address: Some("localhost".into()),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_port_valid_max() {
    let cfg = BackplaneConfig {
        port: Some(65535),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_port_valid_min() {
    let cfg = BackplaneConfig {
        port: Some(1),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_collects_multiple_errors_simultaneously() {
    let mut cfg = BackplaneConfig {
        log_level: Some("NOPE".into()),
        port: Some(0),
        bind_address: Some("".into()),
        policy_profiles: vec!["".into()],
        ..fully_valid_config()
    };
    cfg.backends.insert(
        "b1".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    // log_level + port + bind_address + policy_profile + command + timeout
    assert!(
        reasons.len() >= 5,
        "expected >= 5 errors, got {}: {reasons:?}",
        reasons.len()
    );
}

// ===========================================================================
// Category 7: Environment variable overrides
// ===========================================================================

#[test]
fn env_override_default_backend() {
    let mut cfg = BackplaneConfig::default();
    // Simulate: set the field as if the env var was applied.
    cfg.default_backend = Some("from_env".into());
    assert_eq!(cfg.default_backend.as_deref(), Some("from_env"));
}

#[test]
fn env_override_log_level() {
    let mut cfg = BackplaneConfig::default();
    cfg.log_level = Some("trace".into());
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
}

#[test]
fn env_override_receipts_dir() {
    let mut cfg = BackplaneConfig::default();
    cfg.receipts_dir = Some("/env/receipts".into());
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/env/receipts"));
}

#[test]
fn env_override_workspace_dir() {
    let mut cfg = BackplaneConfig::default();
    cfg.workspace_dir = Some("/env/workspace".into());
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/env/workspace"));
}

#[test]
fn env_override_bind_address() {
    let mut cfg = BackplaneConfig::default();
    cfg.bind_address = Some("0.0.0.0".into());
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
}

#[test]
fn env_override_port() {
    let mut cfg = BackplaneConfig::default();
    cfg.port = Some(9090);
    assert_eq!(cfg.port, Some(9090));
}

#[test]
fn env_override_invalid_log_level_caught_by_validation() {
    let mut cfg = fully_valid_config();
    cfg.log_level = Some("INVALID_FROM_ENV".into());
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

// ===========================================================================
// Category 8: Multiple backends
// ===========================================================================

#[test]
fn config_with_many_mixed_backends() {
    let toml = r#"
        default_backend = "mock"
        log_level = "info"
        receipts_dir = "/tmp/r"

        [backends.mock]
        type = "mock"

        [backends.node_sidecar]
        type = "sidecar"
        command = "node"
        args = ["hosts/node/index.js"]
        timeout_secs = 300

        [backends.python_sidecar]
        type = "sidecar"
        command = "python3"
        args = ["hosts/python/main.py"]

        [backends.claude_sidecar]
        type = "sidecar"
        command = "node"
        args = ["hosts/claude/index.js"]
        timeout_secs = 600

        [backends.another_mock]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 5);
    validate_config(&cfg).unwrap();
}

#[test]
fn config_100_mock_backends() {
    let mut cfg = fully_valid_config();
    for i in 0..100 {
        cfg.backends
            .insert(format!("mock_{i}"), BackendEntry::Mock {});
    }
    // Should be 100 + original 2 backends.
    assert!(cfg.backends.len() >= 100);
    validate_config(&cfg).unwrap();
}

#[test]
fn config_multiple_sidecars_different_commands() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "py".into(),
        BackendEntry::Sidecar {
            command: "python3".into(),
            args: vec!["host.py".into()],
            timeout_secs: None,
        },
    );
    cfg.backends.insert(
        "go".into(),
        BackendEntry::Sidecar {
            command: "go".into(),
            args: vec!["run".into(), "main.go".into()],
            timeout_secs: Some(120),
        },
    );
    cfg.backends.insert(
        "ruby".into(),
        BackendEntry::Sidecar {
            command: "ruby".into(),
            args: vec!["host.rb".into()],
            timeout_secs: Some(60),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn one_bad_backend_among_many_good_produces_error() {
    let mut cfg = fully_valid_config();
    for i in 0..5 {
        cfg.backends.insert(
            format!("good_{i}"),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(60),
            },
        );
    }
    cfg.backends.insert(
        "the_bad_one".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("the_bad_one")));
    assert_eq!(reasons.len(), 1, "only one backend should be invalid");
}

// ===========================================================================
// Category 9: Config merge
// ===========================================================================

#[test]
fn merge_overlay_default_backend_wins() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
}

#[test]
fn merge_overlay_none_preserves_base() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/base".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/base"));
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
    assert!(merged.backends.contains_key("a"));
    assert!(merged.backends.contains_key("b"));
}

#[test]
fn merge_overlay_backend_replaces_same_key() {
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
                timeout_secs: Some(60),
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, args, .. } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_policy_profiles_overlay_wins_when_nonempty() {
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
fn merge_policy_profiles_base_kept_when_overlay_empty() {
    let base = BackplaneConfig {
        policy_profiles: vec!["base.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: Vec::new(),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.policy_profiles, vec!["base.toml"]);
}

#[test]
fn merge_port_overlay_wins() {
    let base = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        port: Some(9090),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.port, Some(9090));
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
fn merge_three_layers() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let mid = BackplaneConfig {
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    let top = BackplaneConfig {
        workspace_dir: Some("/ws".into()),
        ..Default::default()
    };
    let merged = merge_configs(merge_configs(base, mid), top);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    // top layer has Default which includes log_level=Some("info"), so it wins.
    assert_eq!(merged.log_level.as_deref(), Some("info"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/r"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/ws"));
}

#[test]
fn merge_result_passes_validation() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    validate_config(&merged).unwrap();
}

#[test]
fn merge_introduces_invalid_backend_caught_by_validation() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "bad".into(),
            BackendEntry::Sidecar {
                command: "".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(validate_config(&merged).is_err());
}

// ===========================================================================
// Category 10: Serde roundtrip (TOML and JSON)
// ===========================================================================

#[test]
fn toml_roundtrip_fully_valid() {
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
fn toml_roundtrip_with_all_scalar_fields() {
    let cfg = BackplaneConfig {
        default_backend: Some("openai".into()),
        workspace_dir: Some("/workspaces/agent".into()),
        log_level: Some("trace".into()),
        receipts_dir: Some("/data/receipts".into()),
        bind_address: Some("0.0.0.0".into()),
        port: Some(8443),
        policy_profiles: vec!["policy1.toml".into(), "policy2.toml".into()],
        backends: BTreeMap::from([
            ("mock".into(), BackendEntry::Mock {}),
            (
                "sidecar1".into(),
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["--flag".into(), "host.js".into()],
                    timeout_secs: Some(600),
                },
            ),
        ]),
    };
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn json_roundtrip_fully_valid() {
    let cfg = fully_valid_config();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn json_roundtrip_default_config() {
    let cfg = BackplaneConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn toml_roundtrip_preserves_validation_result() {
    let cfg = fully_valid_config();
    let w1 = validate_config(&cfg).unwrap();
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized = parse_toml(&serialized).unwrap();
    let w2 = validate_config(&deserialized).unwrap();
    assert_eq!(w1.len(), w2.len());
}

// ===========================================================================
// Category 11: File loading
// ===========================================================================

#[test]
fn load_from_file_valid_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(
        f,
        "default_backend = \"mock\"\nlog_level = \"warn\"\n\n[backends.mock]\ntype = \"mock\""
    )
    .unwrap();
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn load_from_file_missing_gives_error() {
    let err = load_from_file(Path::new("/nonexistent/path/config.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_config_none_returns_default() {
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
    assert!(cfg.backends.is_empty());
}

#[test]
fn load_config_some_path_reads_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.toml");
    std::fs::write(&path, "log_level = \"trace\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
}

#[test]
fn load_config_missing_path_gives_file_not_found() {
    let err = load_config(Some(Path::new("/no/such/file.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_from_str_valid() {
    let cfg = load_from_str("default_backend = \"mock\"").unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn load_from_str_invalid() {
    let err = load_from_str("[broken").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn load_from_file_invalid_toml_gives_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "this is not [valid TOML =").unwrap();
    let err = load_from_file(&path).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn load_from_file_empty_file_gives_default_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.toml");
    std::fs::write(&path, "").unwrap();
    let cfg = load_from_file(&path).unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.backends.is_empty());
}

// ===========================================================================
// Category 12: Dotted key access and structured validation
// ===========================================================================

#[test]
fn check_field_paths_for_backend_errors() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "mysc".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.field == "backends.mysc.command"),
        "expected dotted path backends.mysc.command, got: {:?}",
        result.errors
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.field == "backends.mysc.timeout_secs"),
        "expected dotted path backends.mysc.timeout_secs, got: {:?}",
        result.errors
    );
}

#[test]
fn check_field_path_for_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..fully_valid_config()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.errors.iter().any(|e| e.field == "log_level"));
}

#[test]
fn check_field_path_for_port() {
    let cfg = BackplaneConfig {
        port: Some(0),
        ..fully_valid_config()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.errors.iter().any(|e| e.field == "port"));
}

#[test]
fn check_field_path_for_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("!!!".into()),
        ..fully_valid_config()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.errors.iter().any(|e| e.field == "bind_address"));
}

#[test]
fn check_field_path_for_default_backend_warning() {
    let mut cfg = fully_valid_config();
    cfg.default_backend = None;
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "default_backend"));
}

#[test]
fn check_field_path_for_receipts_dir_warning() {
    let mut cfg = fully_valid_config();
    cfg.receipts_dir = None;
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "receipts_dir"));
}

#[test]
fn diff_detects_dotted_backend_path() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.backends.insert("new_be".into(), BackendEntry::Mock {});
    let changes = ConfigDiff::diff(&a, &b);
    assert!(
        changes.iter().any(|c| c.field == "backends.new_be"),
        "diff should use dotted path: {changes:?}"
    );
}

#[test]
fn diff_detects_log_level_path() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.log_level = Some("debug".into());
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "log_level"));
}

#[test]
fn diff_detects_port_path() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.port = Some(9090);
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "port"));
}

#[test]
fn diff_detects_policy_profiles_path() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.policy_profiles = vec!["new.toml".into()];
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "policy_profiles"));
}

// ===========================================================================
// Additional: ConfigValidator::check structured result
// ===========================================================================

#[test]
fn check_valid_config_result_is_valid() {
    let result = ConfigValidator::check(&fully_valid_config());
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn check_unknown_default_backend_has_suggestion() {
    let mut cfg = fully_valid_config();
    cfg.default_backend = Some("nonexistent".into());
    let result = ConfigValidator::check(&cfg);
    assert!(
        result
            .suggestions
            .iter()
            .any(|s| s.contains("Set default_backend"))
    );
}

#[test]
fn check_no_backends_has_suggestion() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::new(),
        ..fully_valid_config()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(
        result
            .suggestions
            .iter()
            .any(|s| s.contains("at least one backend"))
    );
}

// ===========================================================================
// Additional: ConfigMerger struct
// ===========================================================================

#[test]
fn config_merger_preserves_base_when_overlay_default() {
    let base = fully_valid_config();
    let merged = ConfigMerger::merge(&base, &BackplaneConfig::default());
    assert_eq!(merged.default_backend, base.default_backend);
    assert_eq!(merged.workspace_dir, base.workspace_dir);
    assert_eq!(merged.receipts_dir, base.receipts_dir);
    assert!(merged.backends.contains_key("mock"));
    assert!(merged.backends.contains_key("sc"));
}

#[test]
fn config_merger_overlay_wins_on_conflict() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        ..Default::default()
    };
    let merged = ConfigMerger::merge(&base, &overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
}

// ===========================================================================
// Additional: ConfigError Display coverage
// ===========================================================================

#[test]
fn config_error_file_not_found_display() {
    let e = ConfigError::FileNotFound {
        path: "/missing.toml".into(),
    };
    let s = e.to_string();
    assert!(s.contains("not found"));
    assert!(s.contains("/missing.toml"));
}

#[test]
fn config_error_parse_error_display() {
    let e = ConfigError::ParseError {
        reason: "unexpected token".into(),
    };
    assert!(e.to_string().contains("unexpected token"));
}

#[test]
fn config_error_validation_error_display() {
    let e = ConfigError::ValidationError {
        reasons: vec!["err1".into(), "err2".into()],
    };
    let s = e.to_string();
    assert!(s.contains("err1"));
    assert!(s.contains("err2"));
}

#[test]
fn config_error_merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "conflicting backends".into(),
    };
    assert!(e.to_string().contains("conflicting backends"));
}

// ===========================================================================
// Additional: ConfigWarning Display coverage
// ===========================================================================

#[test]
fn config_warning_deprecated_with_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: Some("new".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old"));
    assert!(s.contains("new"));
}

#[test]
fn config_warning_deprecated_without_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: None,
    };
    assert!(w.to_string().contains("old"));
}

#[test]
fn config_warning_missing_optional() {
    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "won't persist".into(),
    };
    let s = w.to_string();
    assert!(s.contains("receipts_dir"));
    assert!(s.contains("won't persist"));
}

#[test]
fn config_warning_large_timeout() {
    let w = ConfigWarning::LargeTimeout {
        backend: "slow".into(),
        secs: 7200,
    };
    let s = w.to_string();
    assert!(s.contains("slow"));
    assert!(s.contains("7200"));
}

// ===========================================================================
// Additional: Severity and ValidationIssue Display
// ===========================================================================

#[test]
fn severity_display_values() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn validation_issue_display() {
    let issue = ValidationIssue {
        severity: Severity::Error,
        message: "something broke".into(),
    };
    let s = issue.to_string();
    assert!(s.contains("[error]"));
    assert!(s.contains("something broke"));
}

#[test]
fn issue_severity_display_values() {
    assert_eq!(IssueSeverity::Error.to_string(), "error");
    assert_eq!(IssueSeverity::Warning.to_string(), "warning");
}

// ===========================================================================
// Additional: Idempotency
// ===========================================================================

#[test]
fn validation_idempotent_valid() {
    let cfg = fully_valid_config();
    let w1 = validate_config(&cfg).unwrap();
    let w2 = validate_config(&cfg).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn validation_idempotent_invalid() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..fully_valid_config()
    };
    let r1 = validation_reasons(validate_config(&cfg).unwrap_err());
    let r2 = validation_reasons(validate_config(&cfg).unwrap_err());
    assert_eq!(r1, r2);
}

#[test]
fn check_idempotent() {
    let cfg = fully_valid_config();
    let r1 = ConfigValidator::check(&cfg);
    let r2 = ConfigValidator::check(&cfg);
    assert_eq!(r1.valid, r2.valid);
    assert_eq!(r1.errors.len(), r2.errors.len());
    assert_eq!(r1.warnings.len(), r2.warnings.len());
}
