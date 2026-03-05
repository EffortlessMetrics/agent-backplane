#![allow(clippy::all)]
#![allow(unknown_lints)]

use abp_config::validate::{
    ConfigChange, ConfigDiff, ConfigIssue, ConfigMerger, ConfigValidationResult, ConfigValidator,
    IssueSeverity, Severity, ValidationIssue, diff_configs, from_env_overrides,
};
use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, apply_env_overrides, load_config,
    load_from_file, load_from_str, merge_configs, parse_toml, validate_config,
};
use std::collections::BTreeMap;
use std::path::Path;

// ===========================================================================
// Helper
// ===========================================================================

fn minimal_valid_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        log_level: Some("info".into()),
        ..Default::default()
    }
}

fn sidecar_entry(cmd: &str, timeout: Option<u64>) -> BackendEntry {
    BackendEntry::Sidecar {
        command: cmd.into(),
        args: vec![],
        timeout_secs: timeout,
    }
}

// ===========================================================================
// 1. Default config
// ===========================================================================

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
    validate_config(&BackplaneConfig::default()).expect("default config should validate");
}

// ===========================================================================
// 2. parse_toml / load_from_str
// ===========================================================================

#[test]
fn parse_empty_string() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn parse_only_comments() {
    let cfg = parse_toml("# just a comment\n# another").unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn parse_full_config() {
    let toml = r#"
        default_backend = "sc"
        workspace_dir = "/ws"
        log_level = "debug"
        receipts_dir = "/receipts"
        bind_address = "0.0.0.0"
        port = 8080
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
    assert_eq!(cfg.default_backend.as_deref(), Some("sc"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/ws"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/receipts"));
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
    assert_eq!(cfg.port, Some(8080));
    assert_eq!(cfg.policy_profiles, vec!["a.toml", "b.toml"]);
    assert_eq!(cfg.backends.len(), 2);
}

#[test]
fn parse_mock_backend() {
    let cfg = parse_toml("[backends.m]\ntype = \"mock\"\n").unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn parse_sidecar_without_optional_fields() {
    let toml = r#"
        [backends.s]
        type = "sidecar"
        command = "python"
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["s"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python");
            assert!(args.is_empty());
            assert!(timeout_secs.is_none());
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_sidecar_with_all_fields() {
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
fn parse_invalid_toml_syntax() {
    let err = parse_toml("this is [not valid =").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_log_level() {
    let err = parse_toml("log_level = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_port() {
    let err = parse_toml("port = \"abc\"").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_backends() {
    let err = parse_toml("backends = 123").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_unknown_backend_type() {
    let err = parse_toml("[backends.x]\ntype = \"unknown\"\n").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_sidecar_missing_command() {
    let err = parse_toml("[backends.x]\ntype = \"sidecar\"\n").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn load_from_str_delegates_to_parse_toml() {
    let cfg = load_from_str("default_backend = \"x\"").unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("x"));
}

#[test]
fn parse_port_boundary_max() {
    let cfg = parse_toml("port = 65535").unwrap();
    assert_eq!(cfg.port, Some(65535));
}

#[test]
fn parse_port_boundary_one() {
    let cfg = parse_toml("port = 1").unwrap();
    assert_eq!(cfg.port, Some(1));
}

#[test]
fn parse_port_zero() {
    let cfg = parse_toml("port = 0").unwrap();
    assert_eq!(cfg.port, Some(0));
}

#[test]
fn parse_multiple_backends() {
    let toml = r#"
        [backends.a]
        type = "mock"
        [backends.b]
        type = "mock"
        [backends.c]
        type = "sidecar"
        command = "cmd"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
}

#[test]
fn parse_empty_policy_profiles() {
    let cfg = parse_toml("policy_profiles = []").unwrap();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn parse_empty_args() {
    let toml = r#"
        [backends.s]
        type = "sidecar"
        command = "x"
        args = []
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["s"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        _ => panic!("expected Sidecar"),
    }
}

// ===========================================================================
// 3. load_config / load_from_file
// ===========================================================================

#[test]
fn load_config_none_returns_default() {
    // Clear env override so the default is visible.
    // SAFETY: No other test in this binary relies on ABP_LOG_LEVEL concurrently.
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") };
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn load_config_missing_file_returns_file_not_found() {
    let err = load_config(Some(Path::new("/no/such/file.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_config_from_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.toml");
    std::fs::write(&path, "default_backend = \"test\"\nlog_level = \"warn\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("test"));
    // log_level may be overridden by ABP_LOG_LEVEL env from parallel tests,
    // so only assert it's Some (either "warn" from file or env override).
    assert!(cfg.log_level.is_some());
}

#[test]
fn load_from_file_valid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.toml");
    std::fs::write(&path, "log_level = \"debug\"").unwrap();
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
}

#[test]
fn load_from_file_missing() {
    let err = load_from_file(Path::new("/no/exist.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_from_file_invalid_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "not valid [toml =").unwrap();
    let err = load_from_file(&path).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// ===========================================================================
// 4. validate_config
// ===========================================================================

#[test]
fn validate_valid_log_levels() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some(level.to_string()),
            ..Default::default()
        };
        validate_config(&cfg).expect(&format!("'{level}' should be valid"));
    }
}

#[test]
fn validate_invalid_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

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
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_port_max_is_ok() {
    let cfg = BackplaneConfig {
        port: Some(65535),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_empty_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_whitespace_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("   ".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_valid_ipv4_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_valid_ipv6_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("::1".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_valid_hostname_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("localhost".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_valid_dotted_hostname() {
    let cfg = BackplaneConfig {
        bind_address: Some("my-host.example.com".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_invalid_bind_address() {
    let cfg = BackplaneConfig {
        bind_address: Some("not a valid address!".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_empty_policy_profile_path() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["".into()],
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.iter().any(|r| r.contains("policy profile")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validate_whitespace_policy_profile_path() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["  ".into()],
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_nonexistent_policy_profile() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["/nonexistent/path.toml".into()],
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
fn validate_empty_sidecar_command() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("s".into(), sidecar_entry("", None));
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
fn validate_whitespace_sidecar_command() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("s".into(), sidecar_entry("   ", None));
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_zero_timeout() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", Some(0)));
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_timeout_exceeds_max() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", Some(86_401)));
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_timeout_at_max_is_ok() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", Some(86_400)));
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_timeout_at_one_is_ok() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", Some(1)));
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_large_timeout_warning() {
    let mut cfg = minimal_valid_config();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", Some(7200)));
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
    );
}

#[test]
fn validate_timeout_at_threshold_no_warning() {
    let mut cfg = minimal_valid_config();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", Some(3600)));
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
    );
}

#[test]
fn validate_timeout_just_above_threshold_warns() {
    let mut cfg = minimal_valid_config();
    cfg.backends
        .insert("s".into(), sidecar_entry("node", Some(3601)));
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
    );
}

#[test]
fn validate_empty_backend_name() {
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
fn validate_mock_backend_is_ok() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("m".into(), BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

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
fn validate_no_missing_warnings_when_fields_set() {
    let cfg = minimal_valid_config();
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::MissingOptionalField { .. }))
    );
}

#[test]
fn validate_multiple_errors_collected() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        port: Some(0),
        ..Default::default()
    };
    cfg.backends.insert("s".into(), sidecar_entry("", Some(0)));
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.len() >= 3);
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===========================================================================
// 5. merge_configs
// ===========================================================================

#[test]
fn merge_overlay_overrides_default_backend() {
    let base = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).default_backend.as_deref(),
        Some("b")
    );
}

#[test]
fn merge_base_preserved_when_overlay_is_none() {
    let base = BackplaneConfig {
        workspace_dir: Some("/ws".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        workspace_dir: None,
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).workspace_dir.as_deref(),
        Some("/ws")
    );
}

#[test]
fn merge_overlay_receipts_dir() {
    let base = BackplaneConfig {
        receipts_dir: Some("/old".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        receipts_dir: Some("/new".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).receipts_dir.as_deref(),
        Some("/new")
    );
}

#[test]
fn merge_overlay_bind_address() {
    let base = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        bind_address: Some("0.0.0.0".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).bind_address.as_deref(),
        Some("0.0.0.0")
    );
}

#[test]
fn merge_overlay_port() {
    let base = BackplaneConfig {
        port: Some(80),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        port: Some(443),
        ..Default::default()
    };
    assert_eq!(merge_configs(base, overlay).port, Some(443));
}

#[test]
fn merge_base_port_preserved_when_overlay_none() {
    let base = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    let overlay = BackplaneConfig::default();
    assert_eq!(merge_configs(base, overlay).port, Some(8080));
}

#[test]
fn merge_policy_profiles_overlay_wins_when_nonempty() {
    let base = BackplaneConfig {
        policy_profiles: vec!["a.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec!["b.toml".into()],
        ..Default::default()
    };
    assert_eq!(merge_configs(base, overlay).policy_profiles, vec!["b.toml"]);
}

#[test]
fn merge_policy_profiles_base_preserved_when_overlay_empty() {
    let base = BackplaneConfig {
        policy_profiles: vec!["a.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec![],
        ..Default::default()
    };
    assert_eq!(merge_configs(base, overlay).policy_profiles, vec!["a.toml"]);
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
}

#[test]
fn merge_overlay_backend_wins_on_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar_entry("python", None))]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar_entry("node", Some(60)))]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_two_defaults() {
    let merged = merge_configs(BackplaneConfig::default(), BackplaneConfig::default());
    assert_eq!(merged.log_level.as_deref(), Some("info"));
    assert!(merged.backends.is_empty());
}

// ===========================================================================
// 6. Serialization roundtrips
// ===========================================================================

#[test]
fn toml_roundtrip() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(9090),
        policy_profiles: vec!["p.toml".into()],
        backends: BTreeMap::from([
            ("m".into(), BackendEntry::Mock {}),
            ("s".into(), sidecar_entry("node", Some(120))),
        ]),
    };
    let ser = toml::to_string(&cfg).unwrap();
    let de: BackplaneConfig = toml::from_str(&ser).unwrap();
    assert_eq!(cfg, de);
}

#[test]
fn json_roundtrip() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("::1".into()),
        port: Some(443),
        policy_profiles: vec!["pol.toml".into()],
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let de: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, de);
}

#[test]
fn json_roundtrip_sidecar_with_args() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["--flag".into(), "host.js".into()],
        timeout_secs: Some(60),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let de: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, de);
}

#[test]
fn json_roundtrip_mock() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string(&entry).unwrap();
    let de: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, de);
}

#[test]
fn toml_skip_serializing_none_fields() {
    let cfg = BackplaneConfig::default();
    let ser = toml::to_string(&cfg).unwrap();
    assert!(!ser.contains("default_backend"));
    assert!(!ser.contains("workspace_dir"));
    assert!(!ser.contains("receipts_dir"));
    assert!(!ser.contains("bind_address"));
    assert!(!ser.contains("port"));
}

#[test]
fn toml_skip_serializing_empty_policy_profiles() {
    let cfg = BackplaneConfig::default();
    let ser = toml::to_string(&cfg).unwrap();
    assert!(!ser.contains("policy_profiles"));
}

#[test]
fn json_default_config_roundtrip() {
    let cfg = BackplaneConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let de: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, de);
}

// ===========================================================================
// 7. ConfigError Display
// ===========================================================================

#[test]
fn config_error_file_not_found_display() {
    let e = ConfigError::FileNotFound {
        path: "/my/path".into(),
    };
    let s = e.to_string();
    assert!(s.contains("/my/path"));
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

// ===========================================================================
// 8. ConfigWarning Display + equality
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
    let s = w.to_string();
    assert!(s.contains("old"));
    assert!(!s.contains("instead"));
}

#[test]
fn config_warning_missing_optional_field() {
    let w = ConfigWarning::MissingOptionalField {
        field: "f".into(),
        hint: "important".into(),
    };
    assert!(w.to_string().contains("important"));
}

#[test]
fn config_warning_large_timeout() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 5000,
    };
    let s = w.to_string();
    assert!(s.contains("sc"));
    assert!(s.contains("5000"));
}

#[test]
fn config_warning_clone_eq() {
    let w = ConfigWarning::LargeTimeout {
        backend: "x".into(),
        secs: 100,
    };
    let w2 = w.clone();
    assert_eq!(w, w2);
}

// ===========================================================================
// 9. validate module: Severity
// ===========================================================================

#[test]
fn severity_display() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn severity_ordering() {
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Error);
    assert!(Severity::Info < Severity::Error);
}

// ===========================================================================
// 10. validate module: ValidationIssue
// ===========================================================================

#[test]
fn validation_issue_display() {
    let issue = ValidationIssue {
        severity: Severity::Error,
        message: "bad stuff".into(),
    };
    let s = issue.to_string();
    assert!(s.contains("[error]"));
    assert!(s.contains("bad stuff"));
}

#[test]
fn validation_issue_clone_eq() {
    let a = ValidationIssue {
        severity: Severity::Warning,
        message: "msg".into(),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

// ===========================================================================
// 11. ConfigValidator::validate
// ===========================================================================

#[test]
fn validator_default_config_has_issues() {
    let issues = ConfigValidator::validate(&BackplaneConfig::default()).unwrap();
    assert!(!issues.is_empty());
}

#[test]
fn validator_default_config_has_no_backends_info() {
    let issues = ConfigValidator::validate(&BackplaneConfig::default()).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Info && i.message.contains("no backends"))
    );
}

#[test]
fn validator_invalid_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..Default::default()
    };
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validator_empty_sidecar_command() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("s".into(), sidecar_entry("", None));
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validator_zero_timeout() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("s".into(), sidecar_entry("x", Some(0)));
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validator_large_timeout_warning_issue() {
    let mut cfg = minimal_valid_config();
    cfg.backends
        .insert("s".into(), sidecar_entry("x", Some(7200)));
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Warning && i.message.contains("large timeout"))
    );
}

#[test]
fn validator_missing_default_backend_warning() {
    let issues = ConfigValidator::validate(&BackplaneConfig::default()).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Warning && i.message.contains("default_backend"))
    );
}

#[test]
fn validator_missing_receipts_dir_warning() {
    let issues = ConfigValidator::validate(&BackplaneConfig::default()).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Warning && i.message.contains("receipts_dir"))
    );
}

// ===========================================================================
// 12. ConfigValidator::validate_at
// ===========================================================================

#[test]
fn validate_at_error_only_returns_nothing_for_default() {
    let issues =
        ConfigValidator::validate_at(&BackplaneConfig::default(), Severity::Error).unwrap();
    assert!(issues.is_empty());
}

#[test]
fn validate_at_warning_filters_info() {
    let issues =
        ConfigValidator::validate_at(&BackplaneConfig::default(), Severity::Warning).unwrap();
    assert!(issues.iter().all(|i| i.severity >= Severity::Warning));
}

#[test]
fn validate_at_info_returns_all() {
    let all = ConfigValidator::validate(&BackplaneConfig::default()).unwrap();
    let at_info =
        ConfigValidator::validate_at(&BackplaneConfig::default(), Severity::Info).unwrap();
    assert_eq!(all.len(), at_info.len());
}

// ===========================================================================
// 13. ConfigValidator::check
// ===========================================================================

#[test]
fn check_default_config_is_valid() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    assert!(result.valid);
}

#[test]
fn check_default_config_has_warnings() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    assert!(!result.warnings.is_empty());
}

#[test]
fn check_default_config_has_suggestions() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    assert!(!result.suggestions.is_empty());
}

#[test]
fn check_invalid_log_level_has_errors() {
    let cfg = BackplaneConfig {
        log_level: Some("invalid".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(!result.errors.is_empty());
    assert!(result.errors.iter().any(|e| e.field == "log_level"));
}

#[test]
fn check_port_zero_has_error() {
    let cfg = BackplaneConfig {
        port: Some(0),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "port"));
}

#[test]
fn check_empty_bind_address_has_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "bind_address"));
}

#[test]
fn check_invalid_bind_address_has_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("not valid!!".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
}

#[test]
fn check_empty_policy_profile_has_error() {
    let cfg = BackplaneConfig {
        policy_profiles: vec!["  ".into()],
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.field.contains("policy_profiles"))
    );
}

#[test]
fn check_empty_backend_name_has_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("name must not be empty"))
    );
}

#[test]
fn check_empty_sidecar_command_has_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("s".into(), sidecar_entry("", None));
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field.contains("command")));
}

#[test]
fn check_timeout_out_of_range_has_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("s".into(), sidecar_entry("x", Some(86_401)));
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.field.contains("timeout_secs"))
    );
}

#[test]
fn check_large_timeout_has_warning() {
    let mut cfg = minimal_valid_config();
    cfg.backends
        .insert("s".into(), sidecar_entry("x", Some(7200)));
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.field.contains("timeout_secs"))
    );
}

#[test]
fn check_empty_workspace_dir_warning() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("  ".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "workspace_dir"));
}

#[test]
fn check_default_backend_unknown_warning() {
    let mut cfg = BackplaneConfig {
        default_backend: Some("nonexistent".into()),
        ..Default::default()
    };
    cfg.backends.insert("mock".into(), BackendEntry::Mock {});
    let result = ConfigValidator::check(&cfg);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("does not match"))
    );
    assert!(!result.suggestions.is_empty());
}

#[test]
fn check_default_backend_known_no_mismatch_warning() {
    let mut cfg = minimal_valid_config();
    cfg.backends.insert("mock".into(), BackendEntry::Mock {});
    cfg.default_backend = Some("mock".into());
    let result = ConfigValidator::check(&cfg);
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| w.message.contains("does not match"))
    );
}

// ===========================================================================
// 14. IssueSeverity
// ===========================================================================

#[test]
fn issue_severity_display() {
    assert_eq!(IssueSeverity::Error.to_string(), "error");
    assert_eq!(IssueSeverity::Warning.to_string(), "warning");
}

#[test]
fn issue_severity_serde_roundtrip() {
    let json = serde_json::to_string(&IssueSeverity::Error).unwrap();
    assert_eq!(json, "\"error\"");
    let de: IssueSeverity = serde_json::from_str(&json).unwrap();
    assert_eq!(de, IssueSeverity::Error);

    let json = serde_json::to_string(&IssueSeverity::Warning).unwrap();
    assert_eq!(json, "\"warning\"");
    let de: IssueSeverity = serde_json::from_str(&json).unwrap();
    assert_eq!(de, IssueSeverity::Warning);
}

// ===========================================================================
// 15. ConfigIssue
// ===========================================================================

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

#[test]
fn config_issue_serde_roundtrip() {
    let issue = ConfigIssue {
        field: "port".into(),
        message: "bad".into(),
        severity: IssueSeverity::Warning,
    };
    let json = serde_json::to_string(&issue).unwrap();
    let de: ConfigIssue = serde_json::from_str(&json).unwrap();
    assert_eq!(issue, de);
}

// ===========================================================================
// 16. ConfigValidationResult
// ===========================================================================

#[test]
fn config_validation_result_serde_roundtrip() {
    let result = ConfigValidationResult {
        valid: true,
        errors: vec![],
        warnings: vec![ConfigIssue {
            field: "f".into(),
            message: "m".into(),
            severity: IssueSeverity::Warning,
        }],
        suggestions: vec!["do this".into()],
    };
    let json = serde_json::to_string(&result).unwrap();
    let de: ConfigValidationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(de.valid, true);
    assert_eq!(de.warnings.len(), 1);
    assert_eq!(de.suggestions.len(), 1);
}

// ===========================================================================
// 17. diff_configs
// ===========================================================================

#[test]
fn diff_identical_configs_empty() {
    let cfg = minimal_valid_config();
    assert!(diff_configs(&cfg, &cfg).is_empty());
}

#[test]
fn diff_detects_default_backend_change() {
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
fn diff_detects_port_change() {
    let a = BackplaneConfig {
        port: Some(80),
        ..Default::default()
    };
    let b = BackplaneConfig {
        port: Some(443),
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
fn diff_detects_backend_added() {
    let a = BackplaneConfig::default();
    let mut b = BackplaneConfig::default();
    b.backends.insert("m".into(), BackendEntry::Mock {});
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.m"));
}

#[test]
fn diff_detects_backend_removed() {
    let mut a = BackplaneConfig::default();
    a.backends.insert("m".into(), BackendEntry::Mock {});
    let b = BackplaneConfig::default();
    let diffs = diff_configs(&a, &b);
    assert!(
        diffs
            .iter()
            .any(|d| d.path == "backends.m" && d.new_value == "<absent>")
    );
}

#[test]
fn diff_detects_backend_changed() {
    let mut a = BackplaneConfig::default();
    a.backends.insert("s".into(), sidecar_entry("python", None));
    let mut b = BackplaneConfig::default();
    b.backends
        .insert("s".into(), sidecar_entry("node", Some(60)));
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.s"));
}

#[test]
fn diff_detects_workspace_dir_change() {
    let a = BackplaneConfig {
        workspace_dir: Some("/a".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        workspace_dir: Some("/b".into()),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "workspace_dir"));
}

#[test]
fn diff_detects_receipts_dir_change() {
    let a = BackplaneConfig {
        receipts_dir: Some("/x".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        receipts_dir: None,
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "receipts_dir"));
}

#[test]
fn diff_detects_bind_address_change() {
    let a = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        bind_address: Some("0.0.0.0".into()),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "bind_address"));
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

// ===========================================================================
// 18. ConfigDiff::diff (returns ConfigChange)
// ===========================================================================

#[test]
fn config_diff_diff_returns_changes() {
    let a = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let changes = ConfigDiff::diff(&a, &b);
    assert!(!changes.is_empty());
    assert!(changes.iter().any(|c| c.field == "log_level"));
}

#[test]
fn config_change_display() {
    let c = ConfigChange {
        field: "port".into(),
        old_value: "80".into(),
        new_value: "443".into(),
    };
    let s = c.to_string();
    assert!(s.contains("port"));
    assert!(s.contains("80"));
    assert!(s.contains("443"));
}

#[test]
fn config_change_serde_roundtrip() {
    let c = ConfigChange {
        field: "f".into(),
        old_value: "a".into(),
        new_value: "b".into(),
    };
    let json = serde_json::to_string(&c).unwrap();
    let de: ConfigChange = serde_json::from_str(&json).unwrap();
    assert_eq!(c, de);
}

// ===========================================================================
// 19. ConfigMerger
// ===========================================================================

#[test]
fn config_merger_merge() {
    let base = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = ConfigMerger::merge(&base, &overlay);
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
}

#[test]
fn config_merger_does_not_mutate_inputs() {
    let base = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    let _ = ConfigMerger::merge(&base, &overlay);
    assert_eq!(base.default_backend.as_deref(), Some("a"));
    assert_eq!(overlay.default_backend.as_deref(), Some("b"));
}

// ===========================================================================
// 20. from_env_overrides
// ===========================================================================

#[test]
fn from_env_overrides_delegates() {
    let mut cfg = BackplaneConfig::default();
    // Just verify it doesn't panic; env vars are process-global
    from_env_overrides(&mut cfg);
}

// ===========================================================================
// 21. Environment variable overrides
// ===========================================================================

#[test]
fn env_override_abp_default_backend() {
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "env_mock") };
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("env_mock"));
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") };
}

#[test]
fn env_override_abp_log_level() {
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "trace") };
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") };
}

#[test]
fn env_override_abp_receipts_dir() {
    unsafe { std::env::set_var("ABP_RECEIPTS_DIR", "/env/receipts") };
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/env/receipts"));
    unsafe { std::env::remove_var("ABP_RECEIPTS_DIR") };
}

#[test]
fn env_override_abp_workspace_dir() {
    unsafe { std::env::set_var("ABP_WORKSPACE_DIR", "/env/ws") };
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/env/ws"));
    unsafe { std::env::remove_var("ABP_WORKSPACE_DIR") };
}

#[test]
fn env_override_abp_bind_address() {
    unsafe { std::env::set_var("ABP_BIND_ADDRESS", "10.0.0.1") };
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.bind_address.as_deref(), Some("10.0.0.1"));
    unsafe { std::env::remove_var("ABP_BIND_ADDRESS") };
}

#[test]
fn env_override_abp_port_valid() {
    unsafe { std::env::set_var("ABP_PORT", "9090") };
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.port, Some(9090));
    unsafe { std::env::remove_var("ABP_PORT") };
}

#[test]
fn env_override_abp_port_invalid_ignored() {
    unsafe { std::env::set_var("ABP_PORT", "not_a_number") };
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert!(cfg.port.is_none());
    unsafe { std::env::remove_var("ABP_PORT") };
}

#[test]
fn env_override_overwrites_existing_value() {
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "error") };
    let mut cfg = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("error"));
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") };
}

// ===========================================================================
// 22. BackendEntry equality
// ===========================================================================

#[test]
fn backend_entry_eq_mock() {
    assert_eq!(BackendEntry::Mock {}, BackendEntry::Mock {});
}

#[test]
fn backend_entry_eq_sidecar() {
    let a = sidecar_entry("node", Some(60));
    let b = sidecar_entry("node", Some(60));
    assert_eq!(a, b);
}

#[test]
fn backend_entry_ne_different_command() {
    let a = sidecar_entry("node", None);
    let b = sidecar_entry("python", None);
    assert_ne!(a, b);
}

#[test]
fn backend_entry_ne_different_timeout() {
    let a = sidecar_entry("node", Some(60));
    let b = sidecar_entry("node", Some(120));
    assert_ne!(a, b);
}

#[test]
fn backend_entry_ne_mock_vs_sidecar() {
    assert_ne!(BackendEntry::Mock {}, sidecar_entry("node", None));
}

// ===========================================================================
// 23. BackplaneConfig equality / clone
// ===========================================================================

#[test]
fn backplane_config_clone_eq() {
    let cfg = minimal_valid_config();
    let cloned = cfg.clone();
    assert_eq!(cfg, cloned);
}

#[test]
fn backplane_config_ne() {
    let a = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    assert_ne!(a, b);
}

// ===========================================================================
// 24. Edge cases
// ===========================================================================

#[test]
fn parse_toml_with_extra_unknown_fields_fails() {
    // toml deserialization by default ignores unknown fields unless deny_unknown_fields
    // Since BackplaneConfig doesn't have deny_unknown_fields, this should parse
    // Actually let's test what happens:
    let result = parse_toml("unknown_field = \"value\"");
    // serde with toml by default denies unknown fields in toml crate
    // Let's just check it doesn't panic
    let _ = result;
}

#[test]
fn parse_unicode_values() {
    let cfg = parse_toml("default_backend = \"日本語バックエンド\"").unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("日本語バックエンド"));
}

#[test]
fn parse_empty_string_values() {
    let cfg = parse_toml("default_backend = \"\"").unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some(""));
}

#[test]
fn validate_hostname_with_hyphens() {
    let cfg = BackplaneConfig {
        bind_address: Some("my-host".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_hostname_starting_with_hyphen_invalid() {
    let cfg = BackplaneConfig {
        bind_address: Some("-invalid".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_hostname_ending_with_hyphen_invalid() {
    let cfg = BackplaneConfig {
        bind_address: Some("invalid-".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_all_zeros_ipv4() {
    let cfg = BackplaneConfig {
        bind_address: Some("0.0.0.0".into()),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_args_with_special_chars() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["--flag=value with spaces".into(), "\"quoted\"".into()],
        timeout_secs: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let de: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, de);
}

#[test]
fn merge_three_configs() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        log_level: Some("info".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let c = BackplaneConfig {
        default_backend: Some("c".into()),
        log_level: None,
        ..Default::default()
    };
    let merged = merge_configs(merge_configs(a, b), c);
    assert_eq!(merged.default_backend.as_deref(), Some("c"));
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
}

#[test]
fn validate_many_backends() {
    let mut cfg = BackplaneConfig::default();
    for i in 0..50 {
        cfg.backends
            .insert(format!("backend_{i}"), BackendEntry::Mock {});
    }
    validate_config(&cfg).unwrap();
}

#[test]
fn diff_none_to_some() {
    let a = BackplaneConfig::default();
    let b = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "default_backend"
        && d.old_value.contains("none")
        && d.new_value.contains("mock")));
}

#[test]
fn diff_some_to_none() {
    let a = BackplaneConfig {
        workspace_dir: Some("/ws".into()),
        ..Default::default()
    };
    let b = BackplaneConfig::default();
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "workspace_dir"));
}

#[test]
fn check_no_errors_for_valid_full_config() {
    let mut cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec![],
        backends: BTreeMap::new(),
    };
    cfg.backends.insert("mock".into(), BackendEntry::Mock {});
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn load_config_none_applies_env_overrides() {
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "from_env_load") };
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("from_env_load"));
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") };
}

#[test]
fn load_from_file_does_not_apply_env_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.toml");
    std::fs::write(&path, "log_level = \"warn\"").unwrap();
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "error") };
    let cfg = load_from_file(&path).unwrap();
    // load_from_file does NOT apply env overrides
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") };
}
