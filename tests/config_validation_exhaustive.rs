#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

use abp_config::validate::{
    ConfigChange, ConfigDiff, ConfigIssue, ConfigMerger, ConfigValidationResult, ConfigValidator,
    IssueSeverity, Severity, ValidationIssue,
};
use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, apply_env_overrides, load_config,
    load_from_file, load_from_str, merge_configs, parse_toml, validate_config,
};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

// ===================================================================
// Helper factories
// ===================================================================

fn minimal_valid_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        ..Default::default()
    }
}

fn sidecar_entry(cmd: &str, args: &[&str], timeout: Option<u64>) -> BackendEntry {
    BackendEntry::Sidecar {
        command: cmd.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        timeout_secs: timeout,
    }
}

// ===================================================================
// 1. BackplaneConfig construction and defaults
// ===================================================================

#[test]
fn default_config_log_level_is_info() {
    let cfg = BackplaneConfig::default();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
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
fn default_config_has_empty_backends() {
    assert!(BackplaneConfig::default().backends.is_empty());
}

#[test]
fn default_config_passes_validation() {
    validate_config(&BackplaneConfig::default()).expect("default should be valid");
}

#[test]
fn default_config_produces_advisory_warnings() {
    let warnings = validate_config(&BackplaneConfig::default()).unwrap();
    assert!(
        warnings.len() >= 2,
        "expect missing default_backend + receipts_dir warnings"
    );
}

// ===================================================================
// 2. TOML parsing from example file
// ===================================================================

#[test]
fn parse_example_toml_file() {
    let content = std::fs::read_to_string("backplane.example.toml").unwrap();
    let cfg = parse_toml(&content).unwrap();
    assert!(cfg.backends.contains_key("mock"));
    assert!(cfg.backends.contains_key("openai"));
    assert!(cfg.backends.contains_key("anthropic"));
}

#[test]
fn example_toml_mock_is_mock_variant() {
    let content = std::fs::read_to_string("backplane.example.toml").unwrap();
    let cfg = parse_toml(&content).unwrap();
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
}

#[test]
fn example_toml_openai_is_sidecar() {
    let content = std::fs::read_to_string("backplane.example.toml").unwrap();
    let cfg = parse_toml(&content).unwrap();
    match &cfg.backends["openai"] {
        BackendEntry::Sidecar { command, args, .. } => {
            assert_eq!(command, "node");
            assert!(!args.is_empty());
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn example_toml_anthropic_is_sidecar() {
    let content = std::fs::read_to_string("backplane.example.toml").unwrap();
    let cfg = parse_toml(&content).unwrap();
    match &cfg.backends["anthropic"] {
        BackendEntry::Sidecar { command, .. } => {
            assert_eq!(command, "python3");
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn example_toml_passes_validation() {
    let content = std::fs::read_to_string("backplane.example.toml").unwrap();
    let cfg = parse_toml(&content).unwrap();
    validate_config(&cfg).expect("example config should validate");
}

// ===================================================================
// 3. Backend configuration validation
// ===================================================================

#[test]
fn mock_backend_validates() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("m".into(), BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_with_valid_command_validates() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", &["host.js"], None));
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_with_timeout_in_range_validates() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", &[], Some(300)));
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_empty_command_fails_validation() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("bad".into(), sidecar_entry("", &[], None));
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn sidecar_whitespace_command_fails_validation() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("bad".into(), sidecar_entry("   ", &[], None));
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
fn sidecar_zero_timeout_fails_validation() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", &[], Some(0)));
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn sidecar_timeout_exceeds_max_fails() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", &[], Some(86_401)));
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn sidecar_timeout_at_max_boundary_passes() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", &[], Some(86_400)));
    // 86400 > 3600, so it generates a warning but passes
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
    );
}

#[test]
fn sidecar_timeout_at_1_passes() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", &[], Some(1)));
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_large_timeout_produces_warning() {
    let mut cfg = minimal_valid_config();
    cfg.backends
        .insert("slow".into(), sidecar_entry("node", &[], Some(7200)));
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| match w {
        ConfigWarning::LargeTimeout { backend, secs } => backend == "slow" && *secs == 7200,
        _ => false,
    }));
}

#[test]
fn sidecar_timeout_3600_no_large_warning() {
    let mut cfg = minimal_valid_config();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(3600)));
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { backend, .. } if backend == "sc"))
    );
}

#[test]
fn sidecar_timeout_3601_large_warning() {
    let mut cfg = minimal_valid_config();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", &[], Some(3601)));
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { backend, .. } if backend == "sc"))
    );
}

#[test]
fn empty_backend_name_fails_validation() {
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
fn multiple_backends_all_validated() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("m".into(), BackendEntry::Mock {});
    cfg.backends
        .insert("s1".into(), sidecar_entry("node", &["a.js"], Some(60)));
    cfg.backends
        .insert("s2".into(), sidecar_entry("python3", &["b.py"], Some(120)));
    validate_config(&cfg).unwrap();
}

#[test]
fn multiple_invalid_backends_collects_all_errors() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("a".into(), sidecar_entry("", &[], Some(0)));
    cfg.backends
        .insert("b".into(), sidecar_entry("  ", &[], Some(100_000)));
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(
                reasons.len() >= 4,
                "should collect at least 4 errors, got {reasons:?}"
            );
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===================================================================
// 4. Logging configuration
// ===================================================================

#[test]
fn log_level_error_valid() {
    let cfg = BackplaneConfig {
        log_level: Some("error".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn log_level_warn_valid() {
    let cfg = BackplaneConfig {
        log_level: Some("warn".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn log_level_info_valid() {
    let cfg = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn log_level_debug_valid() {
    let cfg = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn log_level_trace_valid() {
    let cfg = BackplaneConfig {
        log_level: Some("trace".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn log_level_none_valid() {
    let cfg = BackplaneConfig {
        log_level: None,
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn log_level_verbose_invalid() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn log_level_uppercase_info_invalid() {
    let cfg = BackplaneConfig {
        log_level: Some("INFO".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn log_level_empty_string_invalid() {
    let cfg = BackplaneConfig {
        log_level: Some("".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn log_level_warning_invalid() {
    let cfg = BackplaneConfig {
        log_level: Some("warning".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

// ===================================================================
// 5. Policy configuration
// ===================================================================

#[test]
fn empty_policy_profiles_valid() {
    let cfg = BackplaneConfig {
        policy_profiles: vec![],
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn empty_string_policy_path_invalid() {
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
fn whitespace_policy_path_invalid() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["   ".into()],
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn nonexistent_policy_path_invalid() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["/nonexistent/policy.toml".into()],
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn existing_policy_path_valid() {
    // Cargo.toml exists and can serve as an existing path
    let cfg = BackplaneConfig {
        policy_profiles: vec!["Cargo.toml".into()],
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

// ===================================================================
// 6. Workspace configuration
// ===================================================================

#[test]
fn workspace_dir_some_value_parses() {
    let toml = r#"workspace_dir = "/tmp/ws""#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/ws"));
}

#[test]
fn workspace_dir_none_by_default() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.workspace_dir.is_none());
}

// ===================================================================
// 7. Daemon configuration (bind_address, port)
// ===================================================================

#[test]
fn valid_ipv4_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_ipv6_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("::1".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_all_interfaces_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("0.0.0.0".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_hostname_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("localhost".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_fqdn_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("my-host.example.com".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn empty_bind_address_invalid() {
    let cfg = BackplaneConfig {
        bind_address: Some("".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn whitespace_bind_address_invalid() {
    let cfg = BackplaneConfig {
        bind_address: Some("   ".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn invalid_bind_address_rejected() {
    let cfg = BackplaneConfig {
        bind_address: Some("not a valid address!".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn hostname_with_leading_dash_invalid() {
    let cfg = BackplaneConfig {
        bind_address: Some("-bad-host".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn hostname_with_trailing_dash_invalid() {
    let cfg = BackplaneConfig {
        bind_address: Some("bad-host-".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn valid_port_1() {
    let cfg = BackplaneConfig {
        port: Some(1),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_port_8080() {
    let cfg = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_port_65535() {
    let cfg = BackplaneConfig {
        port: Some(65535),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn port_zero_invalid() {
    let cfg = BackplaneConfig {
        port: Some(0),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn port_none_valid() {
    let cfg = BackplaneConfig {
        port: None,
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn bind_address_and_port_together() {
    let cfg = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        port: Some(3000),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

// ===================================================================
// 8. Config merge/override logic
// ===================================================================

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
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        log_level: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
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
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.workspace_dir.as_deref(), Some("/new"));
}

#[test]
fn merge_receipts_dir_overlay_wins() {
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
fn merge_combines_disjoint_backends() {
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
fn merge_overlay_backend_wins_on_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar_entry("python", &[], None))]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar_entry("node", &["a.js"], Some(60)))]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_policy_profiles_overlay_replaces_base() {
    let base = BackplaneConfig {
        policy_profiles: vec!["a.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec!["b.toml".into()],
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.policy_profiles, vec!["b.toml"]);
}

#[test]
fn merge_empty_overlay_policy_profiles_preserves_base() {
    let base = BackplaneConfig {
        policy_profiles: vec!["a.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec![],
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.policy_profiles, vec!["a.toml"]);
}

#[test]
fn merge_two_defaults_is_default_like() {
    let merged = merge_configs(BackplaneConfig::default(), BackplaneConfig::default());
    assert_eq!(merged.log_level.as_deref(), Some("info"));
    assert!(merged.backends.is_empty());
}

#[test]
fn config_merger_struct_delegates_to_merge() {
    let base = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    let merged = ConfigMerger::merge(&base, &overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("b"));
}

// ===================================================================
// 9. Environment variable overrides
// ===================================================================

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn env_override_default_backend() {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "envmock") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") };
    assert_eq!(cfg.default_backend.as_deref(), Some("envmock"));
}

#[test]
fn env_override_log_level() {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "trace") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") };
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
}

#[test]
fn env_override_receipts_dir() {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_RECEIPTS_DIR", "/env/receipts") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var("ABP_RECEIPTS_DIR") };
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/env/receipts"));
}

#[test]
fn env_override_workspace_dir() {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_WORKSPACE_DIR", "/env/ws") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var("ABP_WORKSPACE_DIR") };
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/env/ws"));
}

#[test]
fn env_override_bind_address() {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_BIND_ADDRESS", "0.0.0.0") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var("ABP_BIND_ADDRESS") };
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
}

#[test]
fn env_override_port_valid() {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_PORT", "9090") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var("ABP_PORT") };
    assert_eq!(cfg.port, Some(9090));
}

#[test]
fn env_override_port_invalid_ignored() {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut cfg = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_PORT", "not_a_number") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var("ABP_PORT") };
    assert!(cfg.port.is_none());
}

#[test]
fn env_override_replaces_existing_value() {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut cfg = BackplaneConfig {
        default_backend: Some("original".into()),
        ..Default::default()
    };
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "replaced") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") };
    assert_eq!(cfg.default_backend.as_deref(), Some("replaced"));
}

// ===================================================================
// 10. Invalid configurations produce errors
// ===================================================================

#[test]
fn parse_invalid_toml_syntax() {
    let err = parse_toml("this is [not valid =").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_log_level_integer() {
    let err = parse_toml("log_level = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_port_string() {
    let err = parse_toml("port = \"abc\"").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_backends_list() {
    let err = parse_toml("backends = [1, 2, 3]").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_unknown_backend_type() {
    let toml = r#"
        [backends.x]
        type = "unknown"
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_sidecar_missing_command() {
    let toml = r#"
        [backends.x]
        type = "sidecar"
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn validation_multiple_errors_collected() {
    let cfg = BackplaneConfig {
        log_level: Some("INVALID".into()),
        port: Some(0),
        bind_address: Some("".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.len() >= 3, "expected >=3 errors, got {reasons:?}");
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===================================================================
// 11. TOML parsing edge cases
// ===================================================================

#[test]
fn empty_toml_string_parses_to_defaults() {
    let cfg = parse_toml("").unwrap();
    assert_eq!(cfg.default_backend, None);
    assert!(cfg.backends.is_empty());
    assert_eq!(cfg.log_level, None);
}

#[test]
fn toml_with_only_comments_parses() {
    let cfg = parse_toml("# just a comment\n# another one").unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_with_all_fields() {
    let toml = r#"
        default_backend = "mock"
        workspace_dir = "/ws"
        log_level = "debug"
        receipts_dir = "/r"
        bind_address = "127.0.0.1"
        port = 3000
        policy_profiles = ["a.toml", "b.toml"]

        [backends.mock]
        type = "mock"

        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["host.js"]
        timeout_secs = 300
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/ws"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/r"));
    assert_eq!(cfg.bind_address.as_deref(), Some("127.0.0.1"));
    assert_eq!(cfg.port, Some(3000));
    assert_eq!(cfg.policy_profiles.len(), 2);
    assert_eq!(cfg.backends.len(), 2);
}

#[test]
fn toml_roundtrip_serialize_deserialize() {
    let cfg = minimal_valid_config();
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn toml_roundtrip_full_config() {
    let cfg = BackplaneConfig {
        default_backend: Some("sc".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(3000),
        policy_profiles: vec!["p1.toml".into()],
        backends: BTreeMap::from([
            ("m".into(), BackendEntry::Mock {}),
            ("sc".into(), sidecar_entry("node", &["a.js"], Some(60))),
        ]),
    };
    let s = toml::to_string(&cfg).unwrap();
    let d: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, d);
}

#[test]
fn sidecar_with_empty_args_roundtrip() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

// ===================================================================
// 12. File loading
// ===================================================================

#[test]
fn load_from_file_works() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    std::fs::write(&path, "default_backend = \"mock\"\nlog_level = \"warn\"").unwrap();
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn load_from_file_nonexistent() {
    let err = load_from_file(Path::new("/nonexistent/file.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_from_str_works() {
    let cfg = load_from_str("log_level = \"trace\"").unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
}

#[test]
fn load_config_none_returns_default() {
    let cfg = load_config(None).unwrap();
    // log_level may be overridden by ABP_LOG_LEVEL env var, but base is "info"
    assert!(cfg.log_level.is_some());
}

#[test]
fn load_config_some_reads_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    std::fs::write(&path, "default_backend = \"file\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("file"));
}

#[test]
fn load_config_missing_file_error() {
    let err = load_config(Some(Path::new("/nonexistent.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

// ===================================================================
// 13. ConfigError Display
// ===================================================================

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
        reason: "bad syntax".into(),
    };
    assert!(e.to_string().contains("bad syntax"));
}

#[test]
fn config_error_validation_error_display() {
    let e = ConfigError::ValidationError {
        reasons: vec!["a".into(), "b".into()],
    };
    let s = e.to_string();
    assert!(s.contains("validation failed"));
}

#[test]
fn config_error_merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "conflict".into(),
    };
    assert!(e.to_string().contains("conflict"));
}

// ===================================================================
// 14. ConfigWarning Display and variants
// ===================================================================

#[test]
fn warning_deprecated_field_with_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_f".into(),
        suggestion: Some("new_f".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old_f"));
    assert!(s.contains("new_f"));
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
        hint: "do something".into(),
    };
    assert!(w.to_string().contains("do something"));
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
fn config_warning_eq() {
    let a = ConfigWarning::LargeTimeout {
        backend: "x".into(),
        secs: 100,
    };
    let b = ConfigWarning::LargeTimeout {
        backend: "x".into(),
        secs: 100,
    };
    assert_eq!(a, b);
}

// ===================================================================
// 15. Structured validator (ConfigValidator)
// ===================================================================

#[test]
fn validator_default_config_produces_issues() {
    let issues = ConfigValidator::validate(&BackplaneConfig::default()).unwrap();
    assert!(!issues.is_empty());
}

#[test]
fn validator_valid_config_no_errors() {
    let result = ConfigValidator::check(&minimal_valid_config());
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validator_invalid_log_level_error() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..Default::default()
    };
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validator_check_invalid_log_level_not_valid() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "log_level"));
}

#[test]
fn validator_check_port_zero_error() {
    let cfg = BackplaneConfig {
        port: Some(0),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "port"));
}

#[test]
fn validator_check_empty_bind_address_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "bind_address"));
}

#[test]
fn validator_check_empty_backend_command_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("", &[], None));
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.field == "backends.s.command")
    );
}

#[test]
fn validator_check_zero_timeout_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", &[], Some(0)));
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
}

#[test]
fn validator_check_large_timeout_warning() {
    let mut cfg = minimal_valid_config();
    cfg.backends
        .insert("slow".into(), sidecar_entry("node", &[], Some(7200)));
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.field == "backends.slow.timeout_secs")
    );
}

#[test]
fn validator_check_no_backends_suggestion() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    assert!(!result.suggestions.is_empty());
}

#[test]
fn validator_check_default_backend_references_unknown() {
    let cfg = BackplaneConfig {
        default_backend: Some("nonexistent".into()),
        backends: BTreeMap::from([("real".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("does not match"))
    );
}

#[test]
fn validator_check_empty_workspace_dir_warning() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "workspace_dir"));
}

#[test]
fn validator_check_whitespace_workspace_dir_warning() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("   ".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "workspace_dir"));
}

#[test]
fn validator_check_empty_policy_path_error() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["".into()],
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.field.starts_with("policy_profiles"))
    );
}

#[test]
fn validator_validate_at_filters_by_severity() {
    let issues =
        ConfigValidator::validate_at(&BackplaneConfig::default(), Severity::Warning).unwrap();
    assert!(issues.iter().all(|i| i.severity >= Severity::Warning));
}

#[test]
fn validator_validate_at_info_includes_all() {
    let all = ConfigValidator::validate(&BackplaneConfig::default()).unwrap();
    let info_up =
        ConfigValidator::validate_at(&BackplaneConfig::default(), Severity::Info).unwrap();
    assert_eq!(all.len(), info_up.len());
}

// ===================================================================
// 16. ConfigDiff
// ===================================================================

#[test]
fn diff_identical_configs_empty() {
    use abp_config::validate::diff_configs;
    let cfg = minimal_valid_config();
    let diffs = diff_configs(&cfg, &cfg);
    assert!(diffs.is_empty());
}

#[test]
fn diff_default_backend_change() {
    use abp_config::validate::diff_configs;
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "default_backend"));
}

#[test]
fn diff_backend_added() {
    use abp_config::validate::diff_configs;
    let a = BackplaneConfig::default();
    let b = BackplaneConfig {
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.m"));
}

#[test]
fn diff_backend_removed() {
    use abp_config::validate::diff_configs;
    let a = BackplaneConfig {
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let b = BackplaneConfig::default();
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.m"));
}

#[test]
fn diff_port_change() {
    use abp_config::validate::diff_configs;
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
fn diff_policy_profiles_change() {
    use abp_config::validate::diff_configs;
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
fn diff_display_format() {
    use abp_config::validate::diff_configs;
    let a = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    let s = diffs[0].to_string();
    assert!(s.contains("->"));
}

#[test]
fn config_change_via_diff_struct() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    let changes = ConfigDiff::diff(&a, &b);
    assert!(!changes.is_empty());
    assert!(changes[0].field.contains("default_backend"));
}

// ===================================================================
// 17. Severity and IssueSeverity
// ===================================================================

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
        message: "test msg".into(),
    };
    let s = issue.to_string();
    assert!(s.contains("[error]"));
    assert!(s.contains("test msg"));
}

#[test]
fn config_issue_display() {
    let issue = ConfigIssue {
        field: "log_level".into(),
        message: "invalid".into(),
        severity: IssueSeverity::Error,
    };
    let s = issue.to_string();
    assert!(s.contains("[error]"));
    assert!(s.contains("log_level"));
    assert!(s.contains("invalid"));
}

// ===================================================================
// 18. BackendEntry equality and clone
// ===================================================================

#[test]
fn backend_entry_mock_eq() {
    assert_eq!(BackendEntry::Mock {}, BackendEntry::Mock {});
}

#[test]
fn backend_entry_sidecar_eq() {
    let a = sidecar_entry("node", &["a.js"], Some(60));
    let b = sidecar_entry("node", &["a.js"], Some(60));
    assert_eq!(a, b);
}

#[test]
fn backend_entry_sidecar_neq_different_command() {
    let a = sidecar_entry("node", &[], None);
    let b = sidecar_entry("python", &[], None);
    assert_ne!(a, b);
}

#[test]
fn backend_entry_clone() {
    let entry = sidecar_entry("node", &["a.js"], Some(60));
    let cloned = entry.clone();
    assert_eq!(entry, cloned);
}

#[test]
fn backplane_config_clone() {
    let cfg = minimal_valid_config();
    let cloned = cfg.clone();
    assert_eq!(cfg, cloned);
}

// ===================================================================
// 19. Validation result structure
// ===================================================================

#[test]
fn validation_result_valid_when_no_errors() {
    let result = ConfigValidator::check(&minimal_valid_config());
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validation_result_not_valid_with_errors() {
    let cfg = BackplaneConfig {
        log_level: Some("BAD".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(!result.errors.is_empty());
}

#[test]
fn validation_result_has_suggestions_for_empty_config() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    assert!(!result.suggestions.is_empty());
}

#[test]
fn validation_result_missing_default_backend_warning() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    assert!(result.warnings.iter().any(|w| w.field == "default_backend"));
}

#[test]
fn validation_result_missing_receipts_dir_warning() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    assert!(result.warnings.iter().any(|w| w.field == "receipts_dir"));
}

// ===================================================================
// 20. Complex / edge-case scenarios
// ===================================================================

#[test]
fn many_backends_all_validated() {
    let mut cfg = BackplaneConfig::default();
    for i in 0..20 {
        cfg.backends.insert(
            format!("sc{i}"),
            sidecar_entry("node", &[&format!("host{i}.js")], Some(60)),
        );
    }
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_command_with_path_separators_valid() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("/usr/local/bin/node", &[], None));
    validate_config(&cfg).unwrap();
}

#[test]
fn config_with_all_valid_fields_passes() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(3000),
        policy_profiles: vec!["Cargo.toml".into()],
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
    };
    let warnings = validate_config(&cfg).unwrap();
    // No missing-optional-field warnings since both default_backend and receipts_dir are set
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::MissingOptionalField { .. }))
    );
}

#[test]
fn ipv6_all_interfaces_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("::".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn merge_three_configs_chained() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        workspace_dir: Some("/ws".into()),
        ..Default::default()
    };
    let c = BackplaneConfig {
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    let merged = merge_configs(merge_configs(a, b), c);
    assert_eq!(merged.default_backend.as_deref(), Some("a"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/ws"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/r"));
}

#[test]
fn config_change_display() {
    let change = ConfigChange {
        field: "log_level".into(),
        old_value: "info".into(),
        new_value: "debug".into(),
    };
    let s = change.to_string();
    assert!(s.contains("log_level"));
    assert!(s.contains("->"));
}

#[test]
fn validator_no_backends_info_issue() {
    let issues = ConfigValidator::validate(&BackplaneConfig::default()).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Info && i.message.contains("no backends configured"))
    );
}

#[test]
fn validator_empty_backend_name_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}
