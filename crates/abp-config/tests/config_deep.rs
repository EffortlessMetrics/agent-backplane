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
//! Comprehensive tests for the `abp-config` configuration system.
//!
//! Categories:
//!   1. TOML parsing
//!   2. Config merging & precedence
//!   3. Default values
//!   4. Validation errors
//!   5. Backend configuration
//!   6. Policy configuration
//!   7. Workspace configuration
//!   8. Sidecar registration
//!   9. Environment variable overrides
//!  10. Config file discovery / load_from_file
//!  11. Serde roundtrip
//!  12. Edge cases

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use abp_config::validate::{
    ConfigChange, ConfigMerger, ConfigValidationResult, ConfigValidator, IssueSeverity, Severity,
    ValidationIssue, diff_configs, from_env_overrides,
};
use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, apply_env_overrides, load_config,
    load_from_file, load_from_str, merge_configs, parse_toml, validate_config,
};

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a minimal valid config with no warnings.
fn full_config() -> BackplaneConfig {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("mock".into());
    cfg.receipts_dir = Some("/tmp/receipts".into());
    cfg.backends.insert("mock".into(), BackendEntry::Mock {});
    cfg
}

/// Write `content` to a temp file and return (dir, path).
fn write_temp_toml(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    (dir, path)
}

// ===========================================================================
// 1. TOML parsing
// ===========================================================================

#[test]
fn parse_empty_toml_string() {
    let cfg = parse_toml("").unwrap();
    // Parsed TOML has no fields set (unlike Default which sets log_level)
    assert_eq!(cfg.default_backend, None);
    assert_eq!(cfg.log_level, None);
    assert!(cfg.backends.is_empty());
}

#[test]
fn parse_comment_only_toml() {
    let cfg = parse_toml("# just a comment\n# another one\n").unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn parse_all_scalar_fields() {
    let toml = r#"
        default_backend = "openai"
        workspace_dir = "/work"
        log_level = "trace"
        receipts_dir = "/r"
        bind_address = "0.0.0.0"
        port = 8080
        policy_profiles = ["a.toml", "b.toml"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("openai"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/work"));
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/r"));
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
    assert_eq!(cfg.port, Some(8080));
    assert_eq!(cfg.policy_profiles, vec!["a.toml", "b.toml"]);
}

#[test]
fn parse_minimal_config_just_backend() {
    let toml = r#"
        [backends.m]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 1);
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn parse_invalid_toml_syntax() {
    let err = parse_toml("[[[broken").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_port() {
    let err = parse_toml(r#"port = "not_a_number""#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_log_level() {
    let err = parse_toml("log_level = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_backends() {
    let err = parse_toml("backends = 123").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_unknown_backend_type_tag() {
    let err = parse_toml(
        r#"
        [backends.x]
        type = "unknown_type"
        "#,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_sidecar_missing_command() {
    let err = parse_toml(
        r#"
        [backends.x]
        type = "sidecar"
        "#,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_policy_profiles_wrong_type() {
    let err = parse_toml(r#"policy_profiles = "not_an_array""#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_toml_with_inline_comments() {
    let toml = r#"
        default_backend = "mock" # inline comment
        log_level = "debug"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn load_from_str_delegates_to_parse_toml() {
    let cfg = load_from_str(r#"log_level = "warn""#).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

// ===========================================================================
// 2. Config merging & precedence
// ===========================================================================

#[test]
fn merge_overlay_scalar_wins() {
    let base = BackplaneConfig {
        default_backend: Some("a".into()),
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("b".into()),
        log_level: None,
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert_eq!(m.default_backend.as_deref(), Some("b"));
    // overlay log_level is None, so base wins
    assert_eq!(m.log_level.as_deref(), Some("info"));
}

#[test]
fn merge_base_kept_when_overlay_none() {
    let base = BackplaneConfig {
        workspace_dir: Some("/ws".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(9090),
        ..Default::default()
    };
    let overlay = BackplaneConfig::default();
    let m = merge_configs(base, overlay);
    assert_eq!(m.workspace_dir.as_deref(), Some("/ws"));
    assert_eq!(m.receipts_dir.as_deref(), Some("/r"));
    assert_eq!(m.bind_address.as_deref(), Some("127.0.0.1"));
    assert_eq!(m.port, Some(9090));
}

#[test]
fn merge_backends_union() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert!(m.backends.contains_key("a"));
    assert!(m.backends.contains_key("b"));
}

#[test]
fn merge_backend_collision_overlay_wins() {
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
    let m = merge_configs(base, overlay);
    match &m.backends["sc"] {
        BackendEntry::Sidecar { command, args, .. } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
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
    let m = merge_configs(base, overlay);
    assert_eq!(m.policy_profiles, vec!["overlay.toml"]);
}

#[test]
fn merge_policy_profiles_base_kept_when_overlay_empty() {
    let base = BackplaneConfig {
        policy_profiles: vec!["base.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec![],
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert_eq!(m.policy_profiles, vec!["base.toml"]);
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
    let m = merge_configs(base, overlay);
    assert_eq!(m.port, Some(8080));
}

#[test]
fn merge_two_defaults_is_default() {
    let m = merge_configs(BackplaneConfig::default(), BackplaneConfig::default());
    assert_eq!(m.default_backend, None);
    // default has log_level = Some("info")
    assert_eq!(m.log_level.as_deref(), Some("info"));
    assert!(m.backends.is_empty());
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
    let m = merge_configs(base, overlay);
    assert_eq!(m.bind_address.as_deref(), Some("0.0.0.0"));
}

#[test]
fn config_merger_struct_delegates() {
    let base = full_config();
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let m = ConfigMerger::merge(&base, &overlay);
    assert_eq!(m.log_level.as_deref(), Some("debug"));
    assert_eq!(m.default_backend.as_deref(), Some("mock"));
}

// ===========================================================================
// 3. Default values
// ===========================================================================

#[test]
fn default_log_level_is_info() {
    assert_eq!(
        BackplaneConfig::default().log_level.as_deref(),
        Some("info")
    );
}

#[test]
fn default_backend_is_none() {
    assert_eq!(BackplaneConfig::default().default_backend, None);
}

#[test]
fn default_workspace_dir_is_none() {
    assert_eq!(BackplaneConfig::default().workspace_dir, None);
}

#[test]
fn default_receipts_dir_is_none() {
    assert_eq!(BackplaneConfig::default().receipts_dir, None);
}

#[test]
fn default_bind_address_is_none() {
    assert_eq!(BackplaneConfig::default().bind_address, None);
}

#[test]
fn default_port_is_none() {
    assert_eq!(BackplaneConfig::default().port, None);
}

#[test]
fn default_policy_profiles_is_empty() {
    assert!(BackplaneConfig::default().policy_profiles.is_empty());
}

#[test]
fn default_backends_map_is_empty() {
    assert!(BackplaneConfig::default().backends.is_empty());
}

#[test]
fn default_config_passes_validation() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).expect("default should validate");
    // advisory warnings expected for missing optional fields
    assert!(!warnings.is_empty());
}

// ===========================================================================
// 4. Validation errors
// ===========================================================================

#[test]
fn validation_invalid_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validation_port_zero() {
    let cfg = BackplaneConfig {
        port: Some(0),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.iter().any(|r| r.contains("port")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validation_empty_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.iter().any(|r| r.contains("bind_address")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validation_whitespace_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("   ".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validation_invalid_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("not a valid address!!!".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validation_valid_ipv4_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("192.168.1.1".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("valid IPv4 should pass");
}

#[test]
fn validation_valid_ipv6_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("::1".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("valid IPv6 should pass");
}

#[test]
fn validation_valid_hostname_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("localhost".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("localhost should pass");
}

#[test]
fn validation_empty_sidecar_command() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(
                reasons
                    .iter()
                    .any(|r| r.contains("command must not be empty"))
            );
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validation_whitespace_sidecar_command() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "   ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validation_zero_timeout() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
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
fn validation_timeout_exceeds_max() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
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
fn validation_timeout_at_max_is_ok() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    // 86400 > 3600, so it produces a warning but no error
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
    );
}

#[test]
fn validation_timeout_at_one_is_ok() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(1),
        },
    );
    validate_config(&cfg).expect("timeout=1 should be valid");
}

#[test]
fn validation_empty_backend_name() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validation_empty_policy_profile_path() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["".into()],
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(
                reasons
                    .iter()
                    .any(|r| r.contains("policy profile path must not be empty"))
            );
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validation_nonexistent_policy_profile_path() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["/absolutely/does/not/exist.toml".into()],
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.iter().any(|r| r.contains("does not exist")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validation_multiple_errors_collected() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        port: Some(0),
        ..Default::default()
    };
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(
                reasons.len() >= 3,
                "expected >=3 errors, got {}",
                reasons.len()
            );
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===========================================================================
// 5. Backend configuration
// ===========================================================================

#[test]
fn parse_mock_backend() {
    let toml = r#"
        [backends.m]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn parse_sidecar_backend_full() {
    let toml = r#"
        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["--experimental", "host.js"]
        timeout_secs = 120
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["node"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["--experimental", "host.js"]);
            assert_eq!(*timeout_secs, Some(120));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_sidecar_backend_no_timeout() {
    let toml = r#"
        [backends.py]
        type = "sidecar"
        command = "python3"
        args = ["host.py"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["py"] {
        BackendEntry::Sidecar { timeout_secs, .. } => {
            assert_eq!(*timeout_secs, None);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_sidecar_backend_empty_args() {
    let toml = r#"
        [backends.s]
        type = "sidecar"
        command = "node"
        args = []
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["s"] {
        BackendEntry::Sidecar { args, .. } => {
            assert!(args.is_empty());
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_multiple_backends() {
    let toml = r#"
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
    "#;
    let cfg = parse_toml(toml).unwrap();
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
fn backend_entry_mock_equality() {
    assert_eq!(BackendEntry::Mock {}, BackendEntry::Mock {});
}

#[test]
fn backend_entry_sidecar_equality() {
    let a = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["a.js".into()],
        timeout_secs: Some(60),
    };
    let b = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["a.js".into()],
        timeout_secs: Some(60),
    };
    assert_eq!(a, b);
}

#[test]
fn backend_entry_sidecar_inequality() {
    let a = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    };
    let b = BackendEntry::Sidecar {
        command: "python".into(),
        args: vec![],
        timeout_secs: None,
    };
    assert_ne!(a, b);
}

#[test]
fn large_timeout_warning() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7200),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, secs } if backend == "sc" && *secs == 7200
    )));
}

#[test]
fn timeout_at_threshold_no_warning() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3600),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
    );
}

// ===========================================================================
// 6. Policy configuration
// ===========================================================================

#[test]
fn parse_policy_profiles() {
    let toml = r#"policy_profiles = ["profiles/strict.toml", "profiles/lax.toml"]"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.policy_profiles.len(), 2);
    assert_eq!(cfg.policy_profiles[0], "profiles/strict.toml");
    assert_eq!(cfg.policy_profiles[1], "profiles/lax.toml");
}

#[test]
fn parse_empty_policy_profiles() {
    let toml = r#"policy_profiles = []"#;
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn omitted_policy_profiles_defaults_empty() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn policy_profile_with_existing_path_ok() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("policy.toml");
    std::fs::write(&profile, "# policy").unwrap();

    let cfg = BackplaneConfig {
        policy_profiles: vec![profile.to_str().unwrap().to_string()],
        ..Default::default()
    };
    // Validation should pass (no error for existing file)
    let _warnings = validate_config(&cfg).unwrap();
}

// ===========================================================================
// 7. Workspace configuration
// ===========================================================================

#[test]
fn parse_workspace_dir() {
    let toml = r#"workspace_dir = "/tmp/ws""#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/ws"));
}

#[test]
fn omitted_workspace_dir_is_none() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.workspace_dir.is_none());
}

#[test]
fn merge_workspace_dir_overlay_wins() {
    let base = BackplaneConfig {
        workspace_dir: Some("/old".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        workspace_dir: Some("/new".into()),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert_eq!(m.workspace_dir.as_deref(), Some("/new"));
}

#[test]
fn merge_workspace_dir_base_kept_when_overlay_none() {
    let base = BackplaneConfig {
        workspace_dir: Some("/keep".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig::default();
    let m = merge_configs(base, overlay);
    assert_eq!(m.workspace_dir.as_deref(), Some("/keep"));
}

// ===========================================================================
// 8. Sidecar registration (multiple named sidecars)
// ===========================================================================

#[test]
fn register_multiple_sidecars() {
    let toml = r#"
        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["hosts/node/index.js"]

        [backends.python]
        type = "sidecar"
        command = "python3"
        args = ["hosts/python/main.py"]

        [backends.claude]
        type = "sidecar"
        command = "node"
        args = ["hosts/claude/index.js"]
        timeout_secs = 300

        [backends.copilot]
        type = "sidecar"
        command = "node"
        args = ["hosts/copilot/index.js"]
        timeout_secs = 120
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 4);
    for name in &["node", "python", "claude", "copilot"] {
        assert!(cfg.backends.contains_key(*name), "missing backend: {name}");
    }
}

#[test]
fn sidecar_with_custom_args() {
    let toml = r#"
        [backends.custom]
        type = "sidecar"
        command = "/usr/local/bin/my-agent"
        args = ["--port", "3000", "--verbose"]
        timeout_secs = 60
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["custom"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "/usr/local/bin/my-agent");
            assert_eq!(args.len(), 3);
            assert_eq!(*timeout_secs, Some(60));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn mixed_mock_and_sidecar_backends() {
    let toml = r#"
        [backends.mock]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["openai.js"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
    assert!(matches!(
        cfg.backends["openai"],
        BackendEntry::Sidecar { .. }
    ));
}

// ===========================================================================
// 9. Environment variable overrides
// ===========================================================================

// NOTE: All env override tests are consolidated into one test to avoid
// parallel test interference with shared process-level env vars.

#[test]
fn env_override_all_variables() {
    // -- default_backend --
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "env-mock") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("env-mock"));
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") };

    // -- log_level --
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "trace") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") };

    // -- receipts_dir --
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_RECEIPTS_DIR", "/env/receipts") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/env/receipts"));
    unsafe { std::env::remove_var("ABP_RECEIPTS_DIR") };

    // -- workspace_dir --
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_WORKSPACE_DIR", "/env/ws") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/env/ws"));
    unsafe { std::env::remove_var("ABP_WORKSPACE_DIR") };

    // -- bind_address --
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_BIND_ADDRESS", "0.0.0.0") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
    unsafe { std::env::remove_var("ABP_BIND_ADDRESS") };

    // -- port (valid) --
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_PORT", "9999") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.port, Some(9999));
    unsafe { std::env::remove_var("ABP_PORT") };

    // -- port (invalid, silently ignored) --
    let mut cfg = BackplaneConfig::default();
    cfg.port = Some(1234);
    unsafe { std::env::set_var("ABP_PORT", "not_a_number") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.port, Some(1234));
    unsafe { std::env::remove_var("ABP_PORT") };

    // -- env overrides file value --
    let toml = r#"log_level = "info""#;
    let mut cfg = parse_toml(toml).unwrap();
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "debug") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") };

    // -- from_env_overrides helper --
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "error") };
    from_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("error"));
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") };

    // -- load_config applies env overrides --
    let (_dir, path) = write_temp_toml(r#"log_level = "info""#);
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "trace") };
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") };

    // -- load_config with None returns default --
    unsafe {
        std::env::remove_var("ABP_DEFAULT_BACKEND");
        std::env::remove_var("ABP_LOG_LEVEL");
        std::env::remove_var("ABP_RECEIPTS_DIR");
        std::env::remove_var("ABP_WORKSPACE_DIR");
        std::env::remove_var("ABP_BIND_ADDRESS");
        std::env::remove_var("ABP_PORT");
    }
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

// ===========================================================================
// 10. Config file discovery / load_from_file
// ===========================================================================

#[test]
fn load_from_file_reads_disk() {
    let (_dir, path) = write_temp_toml(r#"default_backend = "mock""#);
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn load_from_file_missing_returns_error() {
    let err = load_from_file(Path::new("/nonexistent/backplane.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_config_with_some_path() {
    let (_dir, path) = write_temp_toml(r#"log_level = "warn""#);
    // Use load_from_file to avoid env var interference from parallel tests
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn load_config_missing_path_gives_file_not_found() {
    let err = load_config(Some(Path::new("/no/such/file.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

// ===========================================================================
// 11. Serde roundtrip
// ===========================================================================

#[test]
fn roundtrip_default_config() {
    let cfg = BackplaneConfig::default();
    let s = toml::to_string(&cfg).unwrap();
    let de: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, de);
}

#[test]
fn roundtrip_full_config() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec!["a.toml".into(), "b.toml".into()],
        backends: BTreeMap::from([
            ("mock".into(), BackendEntry::Mock {}),
            (
                "sc".into(),
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(120),
                },
            ),
        ]),
    };
    let s = toml::to_string(&cfg).unwrap();
    let de: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, de);
}

#[test]
fn roundtrip_mock_backend() {
    let entry = BackendEntry::Mock {};
    let toml_str = toml::to_string(&entry).unwrap();
    let de: BackendEntry = toml::from_str(&toml_str).unwrap();
    assert_eq!(entry, de);
}

#[test]
fn roundtrip_sidecar_backend() {
    let entry = BackendEntry::Sidecar {
        command: "python3".into(),
        args: vec!["host.py".into(), "--verbose".into()],
        timeout_secs: Some(300),
    };
    let toml_str = toml::to_string(&entry).unwrap();
    let de: BackendEntry = toml::from_str(&toml_str).unwrap();
    assert_eq!(entry, de);
}

#[test]
fn roundtrip_sidecar_no_optional_fields() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    };
    let toml_str = toml::to_string(&entry).unwrap();
    let de: BackendEntry = toml::from_str(&toml_str).unwrap();
    assert_eq!(entry, de);
}

#[test]
fn json_roundtrip_config() {
    let cfg = full_config();
    let json = serde_json::to_string(&cfg).unwrap();
    let de: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, de);
}

// ===========================================================================
// 12. Edge cases
// ===========================================================================

#[test]
fn empty_string_toml_is_valid() {
    let cfg = parse_toml("").unwrap();
    validate_config(&cfg).expect("empty toml should validate");
}

#[test]
fn whitespace_only_toml_is_valid() {
    let cfg = parse_toml("   \n\t\n  ").unwrap();
    validate_config(&cfg).expect("whitespace toml should validate");
}

#[test]
fn unicode_workspace_dir() {
    let toml = r#"workspace_dir = "/tmp/工作区/données""#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/工作区/données"));
}

#[test]
fn unicode_backend_name() {
    let toml = r#"
        [backends."バックエンド"]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.backends.contains_key("バックエンド"));
}

#[test]
fn unicode_sidecar_args() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["—flag", "données.js"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => {
            assert_eq!(args[1], "données.js");
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn many_backends_config() {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("b_0".into());
    cfg.receipts_dir = Some("/r".into());
    for i in 0..50 {
        cfg.backends.insert(format!("b_{i}"), BackendEntry::Mock {});
    }
    assert_eq!(cfg.backends.len(), 50);
    validate_config(&cfg).expect("many backends should validate");
}

#[test]
fn backends_map_is_sorted() {
    let toml = r#"
        [backends.zebra]
        type = "mock"
        [backends.alpha]
        type = "mock"
        [backends.middle]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    let keys: Vec<&String> = cfg.backends.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn port_max_value() {
    let toml = r#"port = 65535"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.port, Some(65535));
    validate_config(&cfg).expect("port 65535 should be valid");
}

#[test]
fn port_one_is_valid() {
    let cfg = BackplaneConfig {
        port: Some(1),
        ..Default::default()
    };
    validate_config(&cfg).expect("port 1 should be valid");
}

#[test]
fn port_overflow_parse_error() {
    let err = parse_toml("port = 70000").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// -- ConfigError Display ----------------------------------------------------

#[test]
fn config_error_display_file_not_found() {
    let e = ConfigError::FileNotFound {
        path: "/missing.toml".into(),
    };
    let s = e.to_string();
    assert!(s.contains("/missing.toml"));
    assert!(s.contains("not found"));
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
        reasons: vec!["bad port".into(), "bad log".into()],
    };
    let s = e.to_string();
    assert!(s.contains("bad port"));
    assert!(s.contains("bad log"));
}

#[test]
fn config_error_display_merge_conflict() {
    let e = ConfigError::MergeConflict {
        reason: "conflict".into(),
    };
    assert!(e.to_string().contains("conflict"));
}

// -- ConfigWarning Display --------------------------------------------------

#[test]
fn warning_deprecated_field_with_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: Some("new".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old"));
    assert!(s.contains("new"));
}

#[test]
fn warning_deprecated_field_without_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("old"));
    assert!(!s.contains("instead"));
}

#[test]
fn warning_missing_optional_field() {
    let w = ConfigWarning::MissingOptionalField {
        field: "f".into(),
        hint: "important".into(),
    };
    let s = w.to_string();
    assert!(s.contains("f"));
    assert!(s.contains("important"));
}

#[test]
fn warning_large_timeout_display() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 7200,
    };
    let s = w.to_string();
    assert!(s.contains("sc"));
    assert!(s.contains("7200"));
}

#[test]
fn warning_missing_default_backend() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn warning_missing_receipts_dir() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
}

#[test]
fn no_warning_when_defaults_and_receipts_set() {
    let cfg = full_config();
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::MissingOptionalField { .. })),
        "expected no missing-optional warnings for full config"
    );
}

// -- validate module types --------------------------------------------------

#[test]
fn severity_ordering() {
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Error);
}

#[test]
fn severity_display() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn issue_severity_display() {
    assert_eq!(IssueSeverity::Error.to_string(), "error");
    assert_eq!(IssueSeverity::Warning.to_string(), "warning");
}

#[test]
fn validation_issue_display() {
    let issue = ValidationIssue {
        severity: Severity::Error,
        message: "bad config".into(),
    };
    let s = issue.to_string();
    assert!(s.contains("[error]"));
    assert!(s.contains("bad config"));
}

#[test]
fn config_issue_display() {
    let issue = abp_config::validate::ConfigIssue {
        field: "port".into(),
        message: "out of range".into(),
        severity: IssueSeverity::Error,
    };
    let s = issue.to_string();
    assert!(s.contains("[error]"));
    assert!(s.contains("port"));
    assert!(s.contains("out of range"));
}

// -- ConfigValidator (struct-based) -----------------------------------------

#[test]
fn config_validator_validate_default() {
    let issues = ConfigValidator::validate(&BackplaneConfig::default()).unwrap();
    // Should have info/warning issues for no backends, missing optional
    assert!(!issues.is_empty());
}

#[test]
fn config_validator_validate_invalid_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("banana".into()),
        ..Default::default()
    };
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn config_validator_validate_at_filters_severity() {
    let cfg = BackplaneConfig::default();
    let all = ConfigValidator::validate(&cfg).unwrap();
    let warnings_only = ConfigValidator::validate_at(&cfg, Severity::Warning).unwrap();
    assert!(warnings_only.len() <= all.len());
    for issue in &warnings_only {
        assert!(issue.severity >= Severity::Warning);
    }
}

#[test]
fn config_validator_check_valid_result() {
    let cfg = full_config();
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn config_validator_check_invalid_result() {
    let cfg = BackplaneConfig {
        log_level: Some("nope".into()),
        port: Some(0),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(!result.errors.is_empty());
}

#[test]
fn config_validator_check_warns_unmatched_default_backend() {
    let mut cfg = full_config();
    cfg.default_backend = Some("nonexistent".into());
    let result = ConfigValidator::check(&cfg);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("nonexistent"))
    );
    assert!(!result.suggestions.is_empty());
}

#[test]
fn config_validator_check_warns_empty_workspace_dir() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("  ".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "workspace_dir"));
}

#[test]
fn config_validator_check_suggests_adding_backends() {
    let cfg = BackplaneConfig::default();
    let result = ConfigValidator::check(&cfg);
    assert!(result.suggestions.iter().any(|s| s.contains("backend")));
}

// -- ConfigDiff / diff_configs ----------------------------------------------

#[test]
fn diff_identical_configs_empty() {
    let cfg = full_config();
    let diffs = diff_configs(&cfg, &cfg);
    assert!(diffs.is_empty());
}

#[test]
fn diff_detects_log_level_change() {
    let a = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "log_level"));
}

#[test]
fn diff_detects_backend_added() {
    let a = BackplaneConfig::default();
    let mut b = BackplaneConfig::default();
    b.backends.insert("mock".into(), BackendEntry::Mock {});
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.mock"));
}

#[test]
fn diff_detects_backend_removed() {
    let mut a = BackplaneConfig::default();
    a.backends.insert("mock".into(), BackendEntry::Mock {});
    let b = BackplaneConfig::default();
    let diffs = diff_configs(&a, &b);
    assert!(
        diffs
            .iter()
            .any(|d| d.path == "backends.mock" && d.new_value.contains("absent"))
    );
}

#[test]
fn diff_detects_port_change() {
    let a = BackplaneConfig {
        port: Some(3000),
        ..Default::default()
    };
    let b = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "port"));
}

#[test]
fn diff_detects_policy_profiles_change() {
    let a = BackplaneConfig {
        policy_profiles: vec!["a.toml".into()],
        ..Default::default()
    };
    let b = BackplaneConfig {
        policy_profiles: vec!["b.toml".into()],
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "policy_profiles"));
}

#[test]
fn config_diff_display() {
    let d = abp_config::validate::ConfigDiff {
        path: "log_level".into(),
        old_value: "info".into(),
        new_value: "debug".into(),
    };
    let s = d.to_string();
    assert!(s.contains("log_level"));
    assert!(s.contains("info"));
    assert!(s.contains("debug"));
}

#[test]
fn config_change_display() {
    let c = ConfigChange {
        field: "port".into(),
        old_value: "3000".into(),
        new_value: "8080".into(),
    };
    let s = c.to_string();
    assert!(s.contains("port"));
    assert!(s.contains("3000"));
    assert!(s.contains("8080"));
}

#[test]
fn config_diff_to_config_change() {
    let a = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let changes = abp_config::validate::ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "log_level"));
}

// -- Clone / Debug / PartialEq derive checks --------------------------------

#[test]
fn config_clone_equality() {
    let cfg = full_config();
    let cloned = cfg.clone();
    assert_eq!(cfg, cloned);
}

#[test]
fn config_debug_format() {
    let cfg = full_config();
    let dbg = format!("{cfg:?}");
    assert!(dbg.contains("BackplaneConfig"));
}

#[test]
fn backend_entry_debug_format() {
    let entry = BackendEntry::Mock {};
    let dbg = format!("{entry:?}");
    assert!(dbg.contains("Mock"));
}

#[test]
fn config_warning_clone_equality() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 7200,
    };
    let cloned = w.clone();
    assert_eq!(w, cloned);
}

// -- ConfigValidationResult fields ------------------------------------------

#[test]
fn validation_result_serialization() {
    let result = ConfigValidationResult {
        valid: true,
        errors: vec![],
        warnings: vec![],
        suggestions: vec!["add a backend".into()],
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"valid\":true"));
    assert!(json.contains("add a backend"));
}
