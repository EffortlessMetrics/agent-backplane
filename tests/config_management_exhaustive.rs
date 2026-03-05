#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive configuration management tests covering TOML loading, CLI arg
//! parsing, config merging, validation, and serialization roundtrips.

use abp_config::validate::{
    ConfigChange, ConfigDiff, ConfigIssue, ConfigMerger, ConfigValidationResult, ConfigValidator,
    IssueSeverity, Severity, ValidationIssue,
};
use abp_config::{
    apply_env_overrides, load_config, load_from_file, load_from_str, merge_configs, parse_toml,
    validate_config, BackendEntry, BackplaneConfig, ConfigError, ConfigWarning,
};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

// =========================================================================
// Helper utilities
// =========================================================================

fn minimal_valid_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec![],
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
    }
}

fn sidecar_entry(cmd: &str, args: &[&str], timeout: Option<u64>) -> BackendEntry {
    BackendEntry::Sidecar {
        command: cmd.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        timeout_secs: timeout,
    }
}

// =========================================================================
// Module 1: Default config values
// =========================================================================

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
fn default_config_policy_profiles_empty() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn default_config_backends_empty() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.backends.is_empty());
}

#[test]
fn default_config_passes_validation() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).expect("default config should be valid");
    assert!(!warnings.is_empty(), "should have advisory warnings");
}

// =========================================================================
// Module 2: TOML parsing - valid inputs
// =========================================================================

#[test]
fn parse_empty_toml_string() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.backends.is_empty());
    assert!(cfg.default_backend.is_none());
}

#[test]
fn parse_minimal_toml_with_default_backend() {
    let cfg = parse_toml(r#"default_backend = "mock""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn parse_all_scalar_fields() {
    let toml = r#"
        default_backend = "openai"
        workspace_dir = "/work"
        log_level = "debug"
        receipts_dir = "/receipts"
        bind_address = "0.0.0.0"
        port = 9090
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("openai"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/work"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/receipts"));
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
    assert_eq!(cfg.port, Some(9090));
}

#[test]
fn parse_mock_backend() {
    let toml = r#"
        [backends.test]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(
        cfg.backends.get("test"),
        Some(BackendEntry::Mock {})
    ));
}

#[test]
fn parse_sidecar_backend_with_all_fields() {
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
fn parse_sidecar_backend_without_optional_fields() {
    let toml = r#"
        [backends.py]
        type = "sidecar"
        command = "python3"
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["py"] {
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
fn parse_multiple_backends() {
    let toml = r#"
        [backends.mock]
        type = "mock"

        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["host.js"]

        [backends.python]
        type = "sidecar"
        command = "python3"
        args = ["host.py"]
        timeout_secs = 600
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
    assert!(matches!(
        cfg.backends.get("mock"),
        Some(BackendEntry::Mock {})
    ));
    assert!(matches!(
        cfg.backends.get("node"),
        Some(BackendEntry::Sidecar { .. })
    ));
    assert!(matches!(
        cfg.backends.get("python"),
        Some(BackendEntry::Sidecar { .. })
    ));
}

#[test]
fn parse_policy_profiles() {
    let toml = r#"
        policy_profiles = ["policy1.toml", "policy2.toml"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.policy_profiles, vec!["policy1.toml", "policy2.toml"]);
}

#[test]
fn parse_empty_policy_profiles() {
    let toml = r#"policy_profiles = []"#;
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn parse_sidecar_with_empty_args_array() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = []
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

// =========================================================================
// Module 3: TOML parsing - invalid inputs
// =========================================================================

#[test]
fn parse_error_on_broken_toml() {
    let err = parse_toml("this is [not valid toml =").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_on_wrong_type_log_level() {
    let err = parse_toml(r#"log_level = 42"#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_on_wrong_type_port() {
    let err = parse_toml(r#"port = "not_a_number""#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_on_wrong_type_backends() {
    let err = parse_toml(r#"backends = "not_a_table""#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_on_unknown_backend_type() {
    let toml = r#"
        [backends.bad]
        type = "unknown"
        command = "foo"
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_on_missing_sidecar_command() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_on_negative_port() {
    let err = parse_toml(r#"port = -1"#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_on_port_too_large() {
    let err = parse_toml(r#"port = 70000"#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_on_boolean_default_backend() {
    let err = parse_toml(r#"default_backend = true"#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_on_policy_profiles_not_array() {
    let err = parse_toml(r#"policy_profiles = "single_string""#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// =========================================================================
// Module 4: load_from_str convenience wrapper
// =========================================================================

#[test]
fn load_from_str_valid() {
    let cfg = load_from_str(r#"default_backend = "mock""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn load_from_str_invalid() {
    let err = load_from_str("!!!").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn load_from_str_empty() {
    let cfg = load_from_str("").unwrap();
    assert!(cfg.default_backend.is_none());
}

// =========================================================================
// Module 5: File-based loading
// =========================================================================

#[test]
fn load_from_file_valid_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    std::fs::write(&path, r#"default_backend = "mock""#).unwrap();
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn load_from_file_missing_returns_error() {
    let err = load_from_file(Path::new("/nonexistent/file.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_config_none_returns_default() {
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn load_config_some_path_reads_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bp.toml");
    std::fs::write(&path, "log_level = \"warn\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn load_config_missing_path_returns_file_not_found() {
    let err = load_config(Some(Path::new("/no/such/file.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_from_file_complex_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("complex.toml");
    let content = r#"
        default_backend = "node"
        log_level = "trace"
        port = 3000

        [backends.mock]
        type = "mock"

        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["sidecar.js"]
        timeout_secs = 300
    "#;
    std::fs::write(&path, content).unwrap();
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("node"));
    assert_eq!(cfg.port, Some(3000));
    assert_eq!(cfg.backends.len(), 2);
}

// =========================================================================
// Module 6: Validation - log_level
// =========================================================================

#[test]
fn validate_all_valid_log_levels() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some(level.to_string()),
            ..Default::default()
        };
        validate_config(&cfg).unwrap_or_else(|e| panic!("level '{level}' should be valid: {e}"));
    }
}

#[test]
fn validate_invalid_log_level_verbose() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_invalid_log_level_uppercase() {
    let cfg = BackplaneConfig {
        log_level: Some("INFO".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_invalid_log_level_empty_string() {
    let cfg = BackplaneConfig {
        log_level: Some("".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_log_level_none_is_ok() {
    let cfg = BackplaneConfig {
        log_level: None,
        ..Default::default()
    };
    validate_config(&cfg).expect("None log_level should be valid");
}

// =========================================================================
// Module 7: Validation - port
// =========================================================================

#[test]
fn validate_port_zero_is_error() {
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
fn validate_port_one_is_ok() {
    let cfg = BackplaneConfig {
        port: Some(1),
        ..Default::default()
    };
    validate_config(&cfg).expect("port 1 should be valid");
}

#[test]
fn validate_port_max_u16_is_ok() {
    let cfg = BackplaneConfig {
        port: Some(65535),
        ..Default::default()
    };
    validate_config(&cfg).expect("port 65535 should be valid");
}

#[test]
fn validate_port_common_values() {
    for port in [80, 443, 8080, 8443, 3000, 9090] {
        let cfg = BackplaneConfig {
            port: Some(port),
            ..Default::default()
        };
        validate_config(&cfg).unwrap_or_else(|e| panic!("port {port} should be valid: {e}"));
    }
}

#[test]
fn validate_port_none_is_ok() {
    let cfg = BackplaneConfig {
        port: None,
        ..Default::default()
    };
    validate_config(&cfg).expect("None port should be valid");
}

// =========================================================================
// Module 8: Validation - bind_address
// =========================================================================

#[test]
fn validate_bind_address_ipv4() {
    let cfg = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("ipv4 should be valid");
}

#[test]
fn validate_bind_address_ipv4_any() {
    let cfg = BackplaneConfig {
        bind_address: Some("0.0.0.0".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("0.0.0.0 should be valid");
}

#[test]
fn validate_bind_address_ipv6_loopback() {
    let cfg = BackplaneConfig {
        bind_address: Some("::1".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("::1 should be valid");
}

#[test]
fn validate_bind_address_localhost_hostname() {
    let cfg = BackplaneConfig {
        bind_address: Some("localhost".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("localhost should be valid");
}

#[test]
fn validate_bind_address_dotted_hostname() {
    let cfg = BackplaneConfig {
        bind_address: Some("my-host.example.com".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("dotted hostname should be valid");
}

#[test]
fn validate_bind_address_empty_is_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_bind_address_whitespace_only_is_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("   ".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_bind_address_invalid_hostname_with_underscore() {
    let cfg = BackplaneConfig {
        bind_address: Some("bad_host".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_bind_address_invalid_hostname_starting_with_dash() {
    let cfg = BackplaneConfig {
        bind_address: Some("-badhost".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_bind_address_none_is_ok() {
    let cfg = BackplaneConfig {
        bind_address: None,
        ..Default::default()
    };
    validate_config(&cfg).expect("None bind_address should be valid");
}

// =========================================================================
// Module 9: Validation - backends
// =========================================================================

#[test]
fn validate_empty_backend_name_is_error() {
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
fn validate_sidecar_empty_command_is_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("", &[], None));
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons
                .iter()
                .any(|r| r.contains("command must not be empty")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validate_sidecar_whitespace_only_command_is_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("   ", &[], None));
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons
                .iter()
                .any(|r| r.contains("command must not be empty")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validate_sidecar_timeout_zero_is_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(0)));
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_sidecar_timeout_max_plus_one_is_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(86_401)));
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_sidecar_timeout_exactly_max_is_ok() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(86_400)));
    // This should pass but produce a large timeout warning
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. })));
}

#[test]
fn validate_sidecar_timeout_one_is_ok() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(1)));
    validate_config(&cfg).expect("timeout 1 should be valid");
}

#[test]
fn validate_sidecar_large_timeout_produces_warning() {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("sc".into());
    cfg.receipts_dir = Some("/tmp".into());
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(7200)));
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(
        |w| matches!(w, ConfigWarning::LargeTimeout { backend, secs }
            if backend == "sc" && *secs == 7200)
    ));
}

#[test]
fn validate_sidecar_timeout_at_threshold_no_warning() {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("sc".into());
    cfg.receipts_dir = Some("/tmp".into());
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(3600)));
    let warnings = validate_config(&cfg).unwrap();
    // 3600 is exactly at threshold, not above — no large timeout warning
    assert!(!warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. })));
}

#[test]
fn validate_sidecar_timeout_just_above_threshold_produces_warning() {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("sc".into());
    cfg.receipts_dir = Some("/tmp".into());
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(3601)));
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. })));
}

#[test]
fn validate_mock_backend_always_ok() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("m".into(), BackendEntry::Mock {});
    validate_config(&cfg).expect("mock backend should always pass");
}

#[test]
fn validate_valid_sidecar_with_args() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "n".into(),
        sidecar_entry("node", &["--experimental", "host.js"], Some(300)),
    );
    validate_config(&cfg).expect("valid sidecar should pass");
}

// =========================================================================
// Module 10: Validation - missing optional field warnings
// =========================================================================

#[test]
fn validate_missing_default_backend_warning() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn validate_missing_receipts_dir_warning() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
}

#[test]
fn validate_no_missing_warnings_when_all_set() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        receipts_dir: Some("/tmp".into()),
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    assert!(!warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::MissingOptionalField { .. })));
}

// =========================================================================
// Module 11: Validation - policy profiles
// =========================================================================

#[test]
fn validate_empty_policy_profile_path_is_error() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["".into()],
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons
                .iter()
                .any(|r| r.contains("policy profile path must not be empty")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validate_whitespace_policy_profile_path_is_error() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["   ".into()],
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_nonexistent_policy_profile_is_error() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["/nonexistent/policy.toml".into()],
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons
                .iter()
                .any(|r| r.contains("policy profile path does not exist")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validate_multiple_errors_reported_together() {
    let mut cfg = BackplaneConfig {
        log_level: Some("INVALID".into()),
        port: Some(0),
        ..Default::default()
    };
    cfg.backends
        .insert("sc".into(), sidecar_entry("", &[], None));
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(
                reasons.len() >= 3,
                "should have multiple errors: {reasons:?}"
            );
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// =========================================================================
// Module 12: Config merging
// =========================================================================

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
fn merge_base_preserved_when_overlay_none() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/work".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/work"));
}

#[test]
fn merge_overlay_log_level_wins() {
    let base = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
}

#[test]
fn merge_overlay_port_wins() {
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
fn merge_overlay_bind_address_wins() {
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
fn merge_overlay_receipts_dir_wins() {
    let base = BackplaneConfig {
        receipts_dir: Some("/old".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        receipts_dir: Some("/new".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.receipts_dir.as_deref(), Some("/new"));
}

#[test]
fn merge_overlay_workspace_dir_wins() {
    let base = BackplaneConfig {
        workspace_dir: Some("/old".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        workspace_dir: Some("/new".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.workspace_dir.as_deref(), Some("/new"));
}

#[test]
fn merge_combines_distinct_backends() {
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
    assert_eq!(merged.backends.len(), 2);
}

#[test]
fn merge_overlay_backend_wins_on_key_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar_entry("python", &[], None))]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar_entry("node", &["host.js"], Some(60)))]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_policy_profiles_overlay_wins_when_non_empty() {
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
fn merge_policy_profiles_base_preserved_when_overlay_empty() {
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
fn merge_two_defaults_equals_default() {
    let merged = merge_configs(BackplaneConfig::default(), BackplaneConfig::default());
    assert_eq!(
        merged.default_backend,
        BackplaneConfig::default().default_backend
    );
    assert!(merged.backends.is_empty());
}

#[test]
fn merge_is_not_commutative_for_some_values() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    let ab = merge_configs(a.clone(), b.clone());
    let ba = merge_configs(b, a);
    assert_eq!(ab.default_backend.as_deref(), Some("b"));
    assert_eq!(ba.default_backend.as_deref(), Some("a"));
}

// =========================================================================
// Module 13: ConfigMerger struct wrapper
// =========================================================================

#[test]
fn config_merger_merge_equivalent_to_free_fn() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let via_fn = merge_configs(base.clone(), overlay.clone());
    let via_struct = ConfigMerger::merge(&base, &overlay);
    assert_eq!(via_fn, via_struct);
}

// =========================================================================
// Module 14: Serialization roundtrips
// =========================================================================

#[test]
fn toml_roundtrip_minimal() {
    let cfg = BackplaneConfig::default();
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg.log_level, deserialized.log_level);
}

#[test]
fn toml_roundtrip_full_config() {
    let cfg = minimal_valid_config();
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn toml_roundtrip_with_sidecar() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "node".into(),
        sidecar_entry("node", &["--experimental", "host.js"], Some(300)),
    );
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn json_roundtrip() {
    let cfg = minimal_valid_config();
    let json_str = serde_json::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = serde_json::from_str(&json_str).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn json_roundtrip_with_sidecar_backend() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        sidecar_entry("python3", &["sidecar.py", "--verbose"], Some(600)),
    );
    let json_str = serde_json::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = serde_json::from_str(&json_str).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn json_roundtrip_multiple_backends() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("mock".into(), BackendEntry::Mock {});
    cfg.backends
        .insert("node".into(), sidecar_entry("node", &["host.js"], None));
    cfg.backends.insert(
        "python".into(),
        sidecar_entry("python3", &["host.py"], Some(120)),
    );
    let json_str = serde_json::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = serde_json::from_str(&json_str).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn toml_serialization_skips_none_fields() {
    let cfg = BackplaneConfig::default();
    let serialized = toml::to_string(&cfg).unwrap();
    assert!(!serialized.contains("default_backend"));
    assert!(!serialized.contains("workspace_dir"));
    assert!(!serialized.contains("receipts_dir"));
    assert!(!serialized.contains("bind_address"));
    assert!(!serialized.contains("port"));
}

#[test]
fn toml_serialization_skips_empty_policy_profiles() {
    let cfg = BackplaneConfig::default();
    let serialized = toml::to_string(&cfg).unwrap();
    assert!(!serialized.contains("policy_profiles"));
}

// =========================================================================
// Module 15: ConfigError Display trait
// =========================================================================

#[test]
fn config_error_file_not_found_display() {
    let e = ConfigError::FileNotFound {
        path: "/foo/bar.toml".into(),
    };
    let s = e.to_string();
    assert!(s.contains("/foo/bar.toml"));
    assert!(s.contains("not found"));
}

#[test]
fn config_error_parse_error_display() {
    let e = ConfigError::ParseError {
        reason: "unexpected token".into(),
    };
    let s = e.to_string();
    assert!(s.contains("unexpected token"));
}

#[test]
fn config_error_validation_error_display() {
    let e = ConfigError::ValidationError {
        reasons: vec!["bad port".into(), "bad level".into()],
    };
    let s = e.to_string();
    assert!(s.contains("bad port"));
    assert!(s.contains("bad level"));
}

#[test]
fn config_error_merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "incompatible values".into(),
    };
    let s = e.to_string();
    assert!(s.contains("incompatible values"));
}

// =========================================================================
// Module 16: ConfigWarning Display trait
// =========================================================================

#[test]
fn config_warning_deprecated_field_display_with_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: Some("new_field".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old_field"));
    assert!(s.contains("new_field"));
}

#[test]
fn config_warning_deprecated_field_display_without_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("old_field"));
    assert!(!s.contains("instead"));
}

#[test]
fn config_warning_missing_optional_field_display() {
    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "receipts will not be persisted".into(),
    };
    let s = w.to_string();
    assert!(s.contains("receipts_dir"));
    assert!(s.contains("receipts will not be persisted"));
}

#[test]
fn config_warning_large_timeout_display() {
    let w = ConfigWarning::LargeTimeout {
        backend: "slow_one".into(),
        secs: 9999,
    };
    let s = w.to_string();
    assert!(s.contains("slow_one"));
    assert!(s.contains("9999"));
}

// =========================================================================
// Module 17: ConfigValidator (struct-based)
// =========================================================================

#[test]
fn config_validator_default_produces_issues() {
    let cfg = BackplaneConfig::default();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(!issues.is_empty());
}

#[test]
fn config_validator_invalid_log_level_is_error() {
    let cfg = BackplaneConfig {
        log_level: Some("WRONG".into()),
        ..Default::default()
    };
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn config_validator_empty_backends_produces_info() {
    let cfg = BackplaneConfig::default();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(issues
        .iter()
        .any(|i| i.severity == Severity::Info && i.message.contains("no backends")));
}

#[test]
fn config_validator_missing_default_backend_warning() {
    let cfg = BackplaneConfig::default();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(issues
        .iter()
        .any(|i| i.severity == Severity::Warning && i.message.contains("default_backend")));
}

#[test]
fn config_validator_missing_receipts_dir_warning() {
    let cfg = BackplaneConfig::default();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(issues
        .iter()
        .any(|i| i.severity == Severity::Warning && i.message.contains("receipts_dir")));
}

#[test]
fn config_validator_large_timeout_warning() {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("sc".into());
    cfg.receipts_dir = Some("/tmp".into());
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(7200)));
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(issues
        .iter()
        .any(|i| i.severity == Severity::Warning && i.message.contains("large timeout")));
}

#[test]
fn config_validator_validate_at_filters_by_severity() {
    let cfg = BackplaneConfig::default();
    let all_issues = ConfigValidator::validate(&cfg).unwrap();
    let warnings_only = ConfigValidator::validate_at(&cfg, Severity::Warning).unwrap();
    assert!(warnings_only.len() <= all_issues.len());
    assert!(warnings_only
        .iter()
        .all(|i| i.severity >= Severity::Warning));
}

#[test]
fn config_validator_validate_at_error_level_returns_nothing_for_valid() {
    let cfg = BackplaneConfig::default();
    let errors_only = ConfigValidator::validate_at(&cfg, Severity::Error).unwrap();
    assert!(errors_only.is_empty());
}

// =========================================================================
// Module 18: ConfigValidator::check
// =========================================================================

#[test]
fn check_default_config_is_valid() {
    let cfg = BackplaneConfig::default();
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result.errors.is_empty());
    assert!(!result.warnings.is_empty());
}

#[test]
fn check_invalid_log_level_produces_error() {
    let cfg = BackplaneConfig {
        log_level: Some("BAD".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| e.field == "log_level" && e.severity == IssueSeverity::Error));
}

#[test]
fn check_port_zero_produces_error() {
    let cfg = BackplaneConfig {
        port: Some(0),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "port"));
}

#[test]
fn check_empty_bind_address_produces_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "bind_address"));
}

#[test]
fn check_invalid_bind_address_produces_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("not_a_valid_host!!!".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "bind_address"));
}

#[test]
fn check_empty_sidecar_command_produces_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("", &[], None));
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| e.field == "backends.sc.command"));
}

#[test]
fn check_timeout_out_of_range_produces_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(0)));
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| e.field == "backends.sc.timeout_secs"));
}

#[test]
fn check_large_timeout_produces_warning() {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("sc".into());
    cfg.receipts_dir = Some("/tmp".into());
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(7200)));
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.field == "backends.sc.timeout_secs" && w.severity == IssueSeverity::Warning));
}

#[test]
fn check_missing_default_backend_produces_warning() {
    let cfg = BackplaneConfig::default();
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "default_backend"));
}

#[test]
fn check_missing_receipts_dir_produces_warning() {
    let cfg = BackplaneConfig::default();
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "receipts_dir"));
}

#[test]
fn check_empty_workspace_dir_produces_warning() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "workspace_dir"));
}

#[test]
fn check_whitespace_workspace_dir_produces_warning() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("   ".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "workspace_dir"));
}

#[test]
fn check_default_backend_references_unknown_backend() {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("nonexistent".into());
    cfg.receipts_dir = Some("/tmp".into());
    cfg.backends.insert("mock".into(), BackendEntry::Mock {});
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| {
        w.field == "default_backend" && w.message.contains("does not match any configured backend")
    }));
}

#[test]
fn check_default_backend_references_unknown_produces_suggestion() {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("nonexistent".into());
    cfg.receipts_dir = Some("/tmp".into());
    cfg.backends.insert("mock".into(), BackendEntry::Mock {});
    let result = ConfigValidator::check(&cfg);
    assert!(result.suggestions.iter().any(|s| s.contains("mock")));
}

#[test]
fn check_no_backends_produces_suggestion() {
    let cfg = BackplaneConfig::default();
    let result = ConfigValidator::check(&cfg);
    assert!(result.suggestions.iter().any(|s| s.contains("backend")));
}

#[test]
fn check_empty_policy_profile_path_produces_error() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["".into()],
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| e.field == "policy_profiles[0]"));
}

#[test]
fn check_valid_config_has_no_errors() {
    let cfg = minimal_valid_config();
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

// =========================================================================
// Module 19: ConfigValidationResult serialization
// =========================================================================

#[test]
fn config_validation_result_json_roundtrip() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    let json_str = serde_json::to_string(&result).unwrap();
    let deserialized: ConfigValidationResult = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.valid, result.valid);
    assert_eq!(deserialized.errors.len(), result.errors.len());
    assert_eq!(deserialized.warnings.len(), result.warnings.len());
}

#[test]
fn config_issue_json_roundtrip() {
    let issue = ConfigIssue {
        field: "log_level".into(),
        message: "invalid".into(),
        severity: IssueSeverity::Error,
    };
    let json_str = serde_json::to_string(&issue).unwrap();
    let deserialized: ConfigIssue = serde_json::from_str(&json_str).unwrap();
    assert_eq!(issue, deserialized);
}

#[test]
fn issue_severity_serializes_as_snake_case() {
    let json_err = serde_json::to_string(&IssueSeverity::Error).unwrap();
    let json_warn = serde_json::to_string(&IssueSeverity::Warning).unwrap();
    assert_eq!(json_err, r#""error""#);
    assert_eq!(json_warn, r#""warning""#);
}

// =========================================================================
// Module 20: Display traits for validate types
// =========================================================================

#[test]
fn severity_display() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn validation_issue_display() {
    let issue = ValidationIssue {
        severity: Severity::Warning,
        message: "something off".into(),
    };
    let s = issue.to_string();
    assert!(s.contains("[warning]"));
    assert!(s.contains("something off"));
}

#[test]
fn issue_severity_display() {
    assert_eq!(IssueSeverity::Error.to_string(), "error");
    assert_eq!(IssueSeverity::Warning.to_string(), "warning");
}

#[test]
fn config_issue_display() {
    let issue = ConfigIssue {
        field: "port".into(),
        message: "too low".into(),
        severity: IssueSeverity::Error,
    };
    let s = issue.to_string();
    assert!(s.contains("[error]"));
    assert!(s.contains("port"));
    assert!(s.contains("too low"));
}

// =========================================================================
// Module 21: Config diffing
// =========================================================================

#[test]
fn diff_identical_configs_empty() {
    let cfg = minimal_valid_config();
    let diffs = abp_config::validate::diff_configs(&cfg, &cfg);
    assert!(diffs.is_empty());
}

#[test]
fn diff_detects_default_backend_change() {
    let a = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        default_backend: Some("openai".into()),
        ..Default::default()
    };
    let diffs = abp_config::validate::diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "default_backend"));
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
    let diffs = abp_config::validate::diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "log_level"));
}

#[test]
fn diff_detects_port_change() {
    let a = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    let b = BackplaneConfig {
        port: Some(9090),
        ..Default::default()
    };
    let diffs = abp_config::validate::diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "port"));
}

#[test]
fn diff_detects_added_backend() {
    let a = BackplaneConfig::default();
    let mut b = BackplaneConfig::default();
    b.backends.insert("mock".into(), BackendEntry::Mock {});
    let diffs = abp_config::validate::diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.mock"));
}

#[test]
fn diff_detects_removed_backend() {
    let mut a = BackplaneConfig::default();
    a.backends.insert("mock".into(), BackendEntry::Mock {});
    let b = BackplaneConfig::default();
    let diffs = abp_config::validate::diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.mock"));
}

#[test]
fn diff_detects_changed_backend() {
    let mut a = BackplaneConfig::default();
    a.backends
        .insert("sc".into(), sidecar_entry("python", &[], None));
    let mut b = BackplaneConfig::default();
    b.backends
        .insert("sc".into(), sidecar_entry("node", &[], None));
    let diffs = abp_config::validate::diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.sc"));
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
    let diffs = abp_config::validate::diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "policy_profiles"));
}

#[test]
fn diff_display_format() {
    let diff = abp_config::validate::ConfigDiff {
        path: "log_level".into(),
        old_value: "info".into(),
        new_value: "debug".into(),
    };
    let s = diff.to_string();
    assert!(s.contains("log_level"));
    assert!(s.contains("info"));
    assert!(s.contains("debug"));
    assert!(s.contains("->"));
}

// =========================================================================
// Module 22: ConfigDiff::diff (ConfigChange wrapper)
// =========================================================================

#[test]
fn config_diff_diff_returns_config_changes() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "default_backend"));
}

#[test]
fn config_change_display() {
    let change = ConfigChange {
        field: "port".into(),
        old_value: "8080".into(),
        new_value: "9090".into(),
    };
    let s = change.to_string();
    assert!(s.contains("port"));
    assert!(s.contains("8080"));
    assert!(s.contains("9090"));
}

#[test]
fn config_change_json_roundtrip() {
    let change = ConfigChange {
        field: "log_level".into(),
        old_value: "info".into(),
        new_value: "debug".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let deserialized: ConfigChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, deserialized);
}

// =========================================================================
// Module 23: Severity ordering
// =========================================================================

#[test]
fn severity_ordering_info_lt_warning() {
    assert!(Severity::Info < Severity::Warning);
}

#[test]
fn severity_ordering_warning_lt_error() {
    assert!(Severity::Warning < Severity::Error);
}

#[test]
fn severity_ordering_info_lt_error() {
    assert!(Severity::Info < Severity::Error);
}

// =========================================================================
// Module 24: BackendEntry equality
// =========================================================================

#[test]
fn backend_entry_mock_eq() {
    assert_eq!(BackendEntry::Mock {}, BackendEntry::Mock {});
}

#[test]
fn backend_entry_sidecar_eq() {
    let a = sidecar_entry("node", &["host.js"], Some(60));
    let b = sidecar_entry("node", &["host.js"], Some(60));
    assert_eq!(a, b);
}

#[test]
fn backend_entry_sidecar_ne_different_command() {
    let a = sidecar_entry("node", &[], None);
    let b = sidecar_entry("python", &[], None);
    assert_ne!(a, b);
}

#[test]
fn backend_entry_sidecar_ne_different_timeout() {
    let a = sidecar_entry("node", &[], Some(60));
    let b = sidecar_entry("node", &[], Some(120));
    assert_ne!(a, b);
}

#[test]
fn backend_entry_mock_ne_sidecar() {
    let mock = BackendEntry::Mock {};
    let sidecar = sidecar_entry("node", &[], None);
    assert_ne!(mock, sidecar);
}

// =========================================================================
// Module 25: BackplaneConfig equality
// =========================================================================

#[test]
fn backplane_config_eq_self() {
    let cfg = minimal_valid_config();
    assert_eq!(cfg, cfg.clone());
}

#[test]
fn backplane_config_ne_different_port() {
    let a = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    let b = BackplaneConfig {
        port: Some(9090),
        ..Default::default()
    };
    assert_ne!(a, b);
}

// =========================================================================
// Module 26: Debug mode config (log_level = "debug")
// =========================================================================

#[test]
fn debug_mode_via_toml() {
    let cfg = parse_toml(r#"log_level = "debug""#).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    validate_config(&cfg).expect("debug log level should be valid");
}

#[test]
fn trace_mode_via_toml() {
    let cfg = parse_toml(r#"log_level = "trace""#).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
    validate_config(&cfg).expect("trace log level should be valid");
}

#[test]
fn debug_mode_via_merge() {
    let base = BackplaneConfig::default();
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
}

// =========================================================================
// Module 27: Workspace config
// =========================================================================

#[test]
fn workspace_dir_from_toml() {
    let cfg = parse_toml(r#"workspace_dir = "/tmp/workspaces""#).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/workspaces"));
}

#[test]
fn workspace_dir_merge_overlay_wins() {
    let base = BackplaneConfig {
        workspace_dir: Some("/old".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        workspace_dir: Some("/new".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.workspace_dir.as_deref(), Some("/new"));
}

#[test]
fn workspace_dir_merge_base_preserved() {
    let base = BackplaneConfig {
        workspace_dir: Some("/work".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig::default();
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.workspace_dir.as_deref(), Some("/work"));
}

// =========================================================================
// Module 28: Backend-specific config sections
// =========================================================================

#[test]
fn multiple_backend_types_coexist() {
    let toml = r#"
        [backends.mock]
        type = "mock"

        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["sidecar.js"]
        timeout_secs = 300

        [backends.python]
        type = "sidecar"
        command = "python3"
        args = ["host.py"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(
        cfg.backends.get("mock"),
        Some(BackendEntry::Mock {})
    ));
    match &cfg.backends["node"] {
        BackendEntry::Sidecar {
            command,
            timeout_secs,
            ..
        } => {
            assert_eq!(command, "node");
            assert_eq!(*timeout_secs, Some(300));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
    match &cfg.backends["python"] {
        BackendEntry::Sidecar {
            command,
            timeout_secs,
            ..
        } => {
            assert_eq!(command, "python3");
            assert!(timeout_secs.is_none());
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn backend_btreemap_is_ordered() {
    let toml = r#"
        [backends.zulu]
        type = "mock"

        [backends.alpha]
        type = "mock"

        [backends.middle]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    let keys: Vec<&String> = cfg.backends.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zulu"]);
}

// =========================================================================
// Module 29: from_env_overrides convenience re-export
// =========================================================================

#[test]
fn from_env_overrides_is_available() {
    // Just verify the function exists and can be called
    let mut cfg = BackplaneConfig::default();
    abp_config::validate::from_env_overrides(&mut cfg);
    // We can't rely on env vars being set, but at least it doesn't panic
}

// =========================================================================
// Module 30: Edge cases and special values
// =========================================================================

#[test]
fn config_with_unicode_backend_name() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("ünïcödé".into(), BackendEntry::Mock {});
    validate_config(&cfg).expect("unicode backend name should be valid");
}

#[test]
fn config_with_very_long_backend_name() {
    let mut cfg = BackplaneConfig::default();
    let long_name = "a".repeat(1000);
    cfg.backends
        .insert(long_name.clone(), BackendEntry::Mock {});
    validate_config(&cfg).expect("long backend name should be valid");
}

#[test]
fn config_with_special_chars_in_workspace_dir() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("/path/with spaces/and-dashes/and_underscores".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("special chars in workspace_dir should be ok");
}

#[test]
fn config_with_many_backends() {
    let mut cfg = BackplaneConfig::default();
    for i in 0..50 {
        cfg.backends
            .insert(format!("backend_{i}"), BackendEntry::Mock {});
    }
    validate_config(&cfg).expect("many backends should be valid");
}

#[test]
fn empty_string_fields_roundtrip_via_toml() {
    let cfg = BackplaneConfig {
        default_backend: Some("".into()),
        ..Default::default()
    };
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg.default_backend, deserialized.default_backend);
}

#[test]
fn config_clone_equals_original() {
    let cfg = minimal_valid_config();
    let cloned = cfg.clone();
    assert_eq!(cfg, cloned);
}

#[test]
fn backplane_config_debug_impl() {
    let cfg = BackplaneConfig::default();
    let debug_str = format!("{cfg:?}");
    assert!(debug_str.contains("BackplaneConfig"));
}

#[test]
fn backend_entry_debug_impl() {
    let mock = BackendEntry::Mock {};
    let sidecar = sidecar_entry("node", &["host.js"], Some(60));
    let mock_debug = format!("{mock:?}");
    let sidecar_debug = format!("{sidecar:?}");
    assert!(mock_debug.contains("Mock"));
    assert!(sidecar_debug.contains("Sidecar"));
    assert!(sidecar_debug.contains("node"));
}

#[test]
fn config_warning_clone_and_eq() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 7200,
    };
    let cloned = w.clone();
    assert_eq!(w, cloned);
}

#[test]
fn validation_issue_clone_and_eq() {
    let issue = ValidationIssue {
        severity: Severity::Warning,
        message: "test".into(),
    };
    let cloned = issue.clone();
    assert_eq!(issue, cloned);
}

#[test]
fn config_diff_clone_and_eq() {
    let diff = abp_config::validate::ConfigDiff {
        path: "field".into(),
        old_value: "old".into(),
        new_value: "new".into(),
    };
    let cloned = diff.clone();
    assert_eq!(diff, cloned);
}
