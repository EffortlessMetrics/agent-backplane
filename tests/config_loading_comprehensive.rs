// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration tests for the `abp-config` crate.
//!
//! Categories covered:
//! 1. BackplaneConfig construction and defaults
//! 2. Config serde (TOML roundtrip)
//! 3. Config validation rules
//! 4. Backend configuration
//! 5. Policy configuration (advisory warnings as proxy)
//! 6. Telemetry / log-level configuration
//! 7. Config merging / overlay
//! 8. Edge cases: missing fields, unknown fields, empty values

use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, load_config, merge_configs,
    parse_toml, validate_config,
};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

// ===========================================================================
// Helpers
// ===========================================================================

/// A fully-specified config that passes validation with zero warnings.
fn full_config() -> BackplaneConfig {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "node".into(),
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
        backends,
        ..Default::default()
    }
}

/// Extract error reasons from a `ConfigError::ValidationError`, panicking otherwise.
fn reasons(err: ConfigError) -> Vec<String> {
    match err {
        ConfigError::ValidationError { reasons } => reasons,
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

/// Build a sidecar entry helper.
fn sidecar(cmd: &str, args: &[&str], timeout: Option<u64>) -> BackendEntry {
    BackendEntry::Sidecar {
        command: cmd.into(),
        args: args.iter().map(|s| (*s).to_string()).collect(),
        timeout_secs: timeout,
    }
}

// ###########################################################################
// 1. BackplaneConfig construction and defaults
// ###########################################################################

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
fn default_config_has_empty_backends() {
    assert!(BackplaneConfig::default().backends.is_empty());
}

#[test]
fn default_config_passes_validation() {
    validate_config(&BackplaneConfig::default()).expect("default must be valid");
}

#[test]
fn default_config_produces_advisory_warnings() {
    let w = validate_config(&BackplaneConfig::default()).unwrap();
    assert!(
        !w.is_empty(),
        "default config should warn about missing optional fields"
    );
}

#[test]
fn default_config_warns_about_default_backend() {
    let w = validate_config(&BackplaneConfig::default()).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn default_config_warns_about_receipts_dir() {
    let w = validate_config(&BackplaneConfig::default()).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
}

#[test]
fn full_config_has_no_warnings() {
    let w = validate_config(&full_config()).unwrap();
    assert!(
        w.is_empty(),
        "fully-specified config should have no warnings: {w:?}"
    );
}

#[test]
fn config_clone_equals_original() {
    let cfg = full_config();
    assert_eq!(cfg, cfg.clone());
}

#[test]
fn config_debug_format_contains_fields() {
    let dbg = format!("{:?}", full_config());
    assert!(dbg.contains("default_backend"));
    assert!(dbg.contains("backends"));
}

// ###########################################################################
// 2. Config serde — TOML roundtrip
// ###########################################################################

#[test]
fn toml_roundtrip_full_config() {
    let cfg = full_config();
    let s = toml::to_string(&cfg).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn toml_roundtrip_default_config() {
    let cfg = BackplaneConfig::default();
    let s = toml::to_string(&cfg).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn toml_roundtrip_mock_only() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let s = toml::to_string(&cfg).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn toml_roundtrip_sidecar_with_all_fields() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            sidecar("python3", &["-u", "host.py"], Some(600)),
        )]),
        ..Default::default()
    };
    let s = toml::to_string(&cfg).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn toml_roundtrip_sidecar_no_timeout() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar("node", &[], None))]),
        ..Default::default()
    };
    let s = toml::to_string(&cfg).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn toml_roundtrip_sidecar_empty_args() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar("node", &[], Some(60)))]),
        ..Default::default()
    };
    let s = toml::to_string(&cfg).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn toml_roundtrip_preserves_validity() {
    let cfg = full_config();
    validate_config(&cfg).unwrap();
    let s = toml::to_string(&cfg).unwrap();
    let back = parse_toml(&s).unwrap();
    validate_config(&back).unwrap();
}

#[test]
fn json_roundtrip_full_config() {
    let cfg = full_config();
    let j = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn json_roundtrip_default_config() {
    let cfg = BackplaneConfig::default();
    let j = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn parse_toml_empty_string() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.backends.is_empty());
    assert!(cfg.default_backend.is_none());
}

#[test]
fn parse_toml_only_scalars() {
    let toml = r#"
        default_backend = "openai"
        workspace_dir = "/ws"
        log_level = "debug"
        receipts_dir = "/receipts"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("openai"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/ws"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/receipts"));
    assert!(cfg.backends.is_empty());
}

#[test]
fn parse_toml_multiple_backends() {
    let toml = r#"
        [backends.m]
        type = "mock"

        [backends.s1]
        type = "sidecar"
        command = "node"
        args = ["host.js"]
        timeout_secs = 120

        [backends.s2]
        type = "sidecar"
        command = "python"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
    assert!(matches!(cfg.backends["s1"], BackendEntry::Sidecar { .. }));
    assert!(matches!(cfg.backends["s2"], BackendEntry::Sidecar { .. }));
}

#[test]
fn parse_toml_invalid_syntax_returns_parse_error() {
    let err = parse_toml("[broken ===").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_toml_wrong_type_for_log_level() {
    let err = parse_toml("log_level = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_toml_wrong_type_for_backends() {
    let err = parse_toml("backends = 123").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_toml_missing_sidecar_command_is_parse_error() {
    let toml = r#"
        [backends.bad]
        type = "sidecar"
        args = []
    "#;
    assert!(parse_toml(toml).is_err());
}

#[test]
fn parse_toml_unknown_backend_type_is_parse_error() {
    let toml = r#"
        [backends.bad]
        type = "unknown_type"
        command = "x"
    "#;
    assert!(parse_toml(toml).is_err());
}

#[test]
fn toml_serialization_skips_none_fields() {
    let cfg = BackplaneConfig::default();
    let s = toml::to_string(&cfg).unwrap();
    assert!(!s.contains("default_backend"));
    assert!(!s.contains("workspace_dir"));
    assert!(!s.contains("receipts_dir"));
}

#[test]
fn toml_serialization_includes_set_fields() {
    let cfg = full_config();
    let s = toml::to_string(&cfg).unwrap();
    assert!(s.contains("default_backend"));
    assert!(s.contains("workspace_dir"));
    assert!(s.contains("log_level"));
    assert!(s.contains("receipts_dir"));
}

// ###########################################################################
// 3. Config validation rules
// ###########################################################################

#[test]
fn valid_log_level_error() {
    let cfg = BackplaneConfig {
        log_level: Some("error".into()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_log_level_warn() {
    let cfg = BackplaneConfig {
        log_level: Some("warn".into()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_log_level_info() {
    let cfg = BackplaneConfig {
        log_level: Some("info".into()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_log_level_debug() {
    let cfg = BackplaneConfig {
        log_level: Some("debug".into()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_log_level_trace() {
    let cfg = BackplaneConfig {
        log_level: Some("trace".into()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn none_log_level_is_valid() {
    let cfg = BackplaneConfig {
        log_level: None,
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn invalid_log_level_verbose() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..full_config()
    };
    let r = reasons(validate_config(&cfg).unwrap_err());
    assert!(r.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn invalid_log_level_uppercase_info() {
    let cfg = BackplaneConfig {
        log_level: Some("INFO".into()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn invalid_log_level_mixed_case() {
    let cfg = BackplaneConfig {
        log_level: Some("Debug".into()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn invalid_log_level_empty_string() {
    let cfg = BackplaneConfig {
        log_level: Some(String::new()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn invalid_log_level_whitespace() {
    let cfg = BackplaneConfig {
        log_level: Some("  ".into()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn invalid_log_level_numeric() {
    let cfg = BackplaneConfig {
        log_level: Some("0".into()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn validation_empty_sidecar_command() {
    let mut cfg = full_config();
    cfg.backends.insert("bad".into(), sidecar("", &[], None));
    let r = reasons(validate_config(&cfg).unwrap_err());
    assert!(r.iter().any(|r| r.contains("command must not be empty")));
}

#[test]
fn validation_whitespace_only_sidecar_command() {
    let mut cfg = full_config();
    cfg.backends.insert("bad".into(), sidecar("   ", &[], None));
    let r = reasons(validate_config(&cfg).unwrap_err());
    assert!(r.iter().any(|r| r.contains("command must not be empty")));
}

#[test]
fn validation_tab_only_sidecar_command() {
    let mut cfg = full_config();
    cfg.backends
        .insert("bad".into(), sidecar("\t\t", &[], None));
    let r = reasons(validate_config(&cfg).unwrap_err());
    assert!(r.iter().any(|r| r.contains("command must not be empty")));
}

#[test]
fn validation_zero_timeout() {
    let mut cfg = full_config();
    cfg.backends
        .insert("bad".into(), sidecar("node", &[], Some(0)));
    let r = reasons(validate_config(&cfg).unwrap_err());
    assert!(r.iter().any(|r| r.contains("out of range")));
}

#[test]
fn validation_timeout_exceeds_max() {
    let mut cfg = full_config();
    cfg.backends
        .insert("bad".into(), sidecar("node", &[], Some(86_401)));
    let r = reasons(validate_config(&cfg).unwrap_err());
    assert!(r.iter().any(|r| r.contains("out of range")));
}

#[test]
fn validation_timeout_u64_max() {
    let mut cfg = full_config();
    cfg.backends
        .insert("bad".into(), sidecar("node", &[], Some(u64::MAX)));
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn validation_timeout_boundary_1_is_valid() {
    let mut cfg = full_config();
    cfg.backends
        .insert("ok".into(), sidecar("node", &[], Some(1)));
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_timeout_boundary_86400_is_valid() {
    let mut cfg = full_config();
    cfg.backends
        .insert("ok".into(), sidecar("node", &[], Some(86_400)));
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_empty_backend_name() {
    let mut cfg = full_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let r = reasons(validate_config(&cfg).unwrap_err());
    assert!(r.iter().any(|r| r.contains("name must not be empty")));
}

#[test]
fn validation_collects_multiple_errors() {
    let mut cfg = BackplaneConfig {
        log_level: Some("INVALID".into()),
        ..full_config()
    };
    cfg.backends.insert("a".into(), sidecar("", &[], Some(0)));
    cfg.backends
        .insert("b".into(), sidecar("  ", &[], Some(100_000)));
    let r = reasons(validate_config(&cfg).unwrap_err());
    // log_level + 2 empty commands + 2 timeout errors = >= 5
    assert!(r.len() >= 5, "expected >= 5 errors, got {}: {r:?}", r.len());
}

#[test]
fn validation_is_idempotent_valid() {
    let cfg = full_config();
    let w1 = validate_config(&cfg).unwrap();
    let w2 = validate_config(&cfg).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn validation_is_idempotent_invalid() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..full_config()
    };
    let r1 = reasons(validate_config(&cfg).unwrap_err());
    let r2 = reasons(validate_config(&cfg).unwrap_err());
    assert_eq!(r1, r2);
}

// ###########################################################################
// 4. Backend configuration
// ###########################################################################

#[test]
fn mock_backend_always_valid() {
    let mut cfg = full_config();
    cfg.backends.insert("m1".into(), BackendEntry::Mock {});
    cfg.backends.insert("m2".into(), BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_with_args_parses() {
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
fn sidecar_default_args_empty() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "python"
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn sidecar_default_timeout_none() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "python"
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert!(timeout_secs.is_none()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn mix_of_mock_and_sidecar() {
    let mut cfg = full_config();
    cfg.backends.insert("m".into(), BackendEntry::Mock {});
    cfg.backends
        .insert("s1".into(), sidecar("node", &[], Some(60)));
    cfg.backends
        .insert("s2".into(), sidecar("python", &["host.py"], None));
    validate_config(&cfg).unwrap();
}

#[test]
fn many_backends_all_valid() {
    let mut cfg = full_config();
    for i in 0..50 {
        cfg.backends
            .insert(format!("mock_{i}"), BackendEntry::Mock {});
    }
    validate_config(&cfg).unwrap();
}

#[test]
fn one_bad_sidecar_among_good_backends() {
    let mut cfg = full_config();
    cfg.backends
        .insert("good".into(), sidecar("node", &[], Some(60)));
    cfg.backends.insert("broken".into(), sidecar("", &[], None));
    let r = reasons(validate_config(&cfg).unwrap_err());
    assert!(r.iter().any(|r| r.contains("broken")));
}

#[test]
fn backend_entry_mock_equality() {
    assert_eq!(BackendEntry::Mock {}, BackendEntry::Mock {});
}

#[test]
fn backend_entry_sidecar_equality() {
    let a = sidecar("node", &["a"], Some(60));
    let b = sidecar("node", &["a"], Some(60));
    assert_eq!(a, b);
}

#[test]
fn backend_entry_sidecar_inequality_command() {
    let a = sidecar("node", &[], None);
    let b = sidecar("python", &[], None);
    assert_ne!(a, b);
}

#[test]
fn backend_entry_sidecar_inequality_args() {
    let a = sidecar("node", &["a"], None);
    let b = sidecar("node", &["b"], None);
    assert_ne!(a, b);
}

#[test]
fn backend_entry_sidecar_inequality_timeout() {
    let a = sidecar("node", &[], Some(60));
    let b = sidecar("node", &[], Some(120));
    assert_ne!(a, b);
}

// ###########################################################################
// 5. Policy configuration — advisory warnings
// ###########################################################################

#[test]
fn missing_default_backend_produces_warning() {
    let cfg = BackplaneConfig {
        default_backend: None,
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    let w = validate_config(&cfg).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn missing_receipts_dir_produces_warning() {
    let cfg = BackplaneConfig {
        default_backend: Some("x".into()),
        receipts_dir: None,
        ..Default::default()
    };
    let w = validate_config(&cfg).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
}

#[test]
fn both_optional_fields_missing_produces_two_warnings() {
    let cfg = BackplaneConfig {
        default_backend: None,
        receipts_dir: None,
        ..Default::default()
    };
    let w = validate_config(&cfg).unwrap();
    let count = w
        .iter()
        .filter(|w| matches!(w, ConfigWarning::MissingOptionalField { .. }))
        .count();
    assert_eq!(count, 2);
}

#[test]
fn setting_default_backend_removes_its_warning() {
    let mut cfg = BackplaneConfig {
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    let w1 = validate_config(&cfg).unwrap();
    assert!(w1.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
    cfg.default_backend = Some("mock".into());
    let w2 = validate_config(&cfg).unwrap();
    assert!(!w2.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn setting_receipts_dir_removes_its_warning() {
    let mut cfg = BackplaneConfig {
        default_backend: Some("m".into()),
        ..Default::default()
    };
    let w1 = validate_config(&cfg).unwrap();
    assert!(w1.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
    cfg.receipts_dir = Some("/r".into());
    let w2 = validate_config(&cfg).unwrap();
    assert!(!w2.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
}

// ###########################################################################
// 6. Telemetry / log-level configuration
// ###########################################################################

#[test]
fn large_timeout_produces_warning() {
    let mut cfg = full_config();
    cfg.backends
        .insert("big".into(), sidecar("node", &[], Some(3_601)));
    let w = validate_config(&cfg).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, secs } if backend == "big" && *secs == 3_601
    )));
}

#[test]
fn exactly_at_threshold_no_large_warning() {
    let mut cfg = full_config();
    cfg.backends
        .insert("exact".into(), sidecar("node", &[], Some(3_600)));
    let w = validate_config(&cfg).unwrap();
    assert!(!w.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "exact"
    )));
}

#[test]
fn just_below_threshold_no_large_warning() {
    let mut cfg = full_config();
    cfg.backends
        .insert("below".into(), sidecar("node", &[], Some(3_599)));
    let w = validate_config(&cfg).unwrap();
    assert!(!w.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "below"
    )));
}

#[test]
fn multiple_large_timeouts_produce_multiple_warnings() {
    let mut cfg = full_config();
    cfg.backends
        .insert("big1".into(), sidecar("node", &[], Some(7_200)));
    cfg.backends
        .insert("big2".into(), sidecar("python", &[], Some(43_200)));
    let w = validate_config(&cfg).unwrap();
    let count = w
        .iter()
        .filter(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
        .count();
    assert_eq!(count, 2);
}

#[test]
fn large_timeout_at_max_is_warning_not_error() {
    let mut cfg = full_config();
    cfg.backends
        .insert("maxed".into(), sidecar("node", &[], Some(86_400)));
    let w = validate_config(&cfg).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, secs } if backend == "maxed" && *secs == 86_400
    )));
}

#[test]
fn config_warning_display_deprecated_with_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: Some("new_field".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old_field"));
    assert!(s.contains("new_field"));
}

#[test]
fn config_warning_display_deprecated_without_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("old"));
    assert!(!s.contains("instead"));
}

#[test]
fn config_warning_display_missing_optional() {
    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "will not be persisted".into(),
    };
    let s = w.to_string();
    assert!(s.contains("receipts_dir"));
    assert!(s.contains("persisted"));
}

#[test]
fn config_warning_display_large_timeout() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 9999,
    };
    let s = w.to_string();
    assert!(s.contains("sc"));
    assert!(s.contains("9999"));
}

#[test]
fn config_warning_clone_eq() {
    let w = ConfigWarning::LargeTimeout {
        backend: "x".into(),
        secs: 5000,
    };
    assert_eq!(w, w.clone());
}

// ###########################################################################
// 7. Config merging / overlay
// ###########################################################################

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
fn merge_overlay_none_keeps_base_default_backend() {
    let base = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).default_backend.as_deref(),
        Some("a")
    );
}

#[test]
fn merge_overlay_overrides_workspace_dir() {
    let base = BackplaneConfig {
        workspace_dir: Some("/old".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        workspace_dir: Some("/new".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).workspace_dir.as_deref(),
        Some("/new")
    );
}

#[test]
fn merge_overlay_none_keeps_base_workspace_dir() {
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
fn merge_overlay_overrides_log_level() {
    let base = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).log_level.as_deref(),
        Some("debug")
    );
}

#[test]
fn merge_overlay_none_keeps_base_log_level() {
    let base = BackplaneConfig {
        log_level: Some("warn".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        log_level: None,
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).log_level.as_deref(),
        Some("warn")
    );
}

#[test]
fn merge_overlay_overrides_receipts_dir() {
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
fn merge_overlay_none_keeps_base_receipts_dir() {
    let base = BackplaneConfig {
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        receipts_dir: None,
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).receipts_dir.as_deref(),
        Some("/r")
    );
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
    let m = merge_configs(base, overlay);
    assert!(m.backends.contains_key("a"));
    assert!(m.backends.contains_key("b"));
}

#[test]
fn merge_overlay_backend_wins_on_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar("python", &[], None))]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar("node", &["host.js"], Some(60)))]),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    match &m.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_preserves_base_when_overlay_is_empty() {
    let base = full_config();
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let m = merge_configs(base.clone(), overlay);
    assert_eq!(m.default_backend, base.default_backend);
    assert_eq!(m.workspace_dir, base.workspace_dir);
    assert_eq!(m.log_level, base.log_level);
    assert_eq!(m.receipts_dir, base.receipts_dir);
    assert_eq!(m.backends, base.backends);
}

#[test]
fn merge_two_defaults_is_default() {
    let a = BackplaneConfig::default();
    let b = BackplaneConfig::default();
    let m = merge_configs(a.clone(), b);
    // Default has log_level = Some("info"), so merged should too
    assert_eq!(m.log_level.as_deref(), Some("info"));
    assert!(m.backends.is_empty());
}

#[test]
fn merge_chain_three_configs() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        workspace_dir: Some("/b".into()),
        ..Default::default()
    };
    let c = BackplaneConfig {
        receipts_dir: Some("/c".into()),
        ..Default::default()
    };
    let m = merge_configs(merge_configs(a, b), c);
    assert_eq!(m.default_backend.as_deref(), Some("a"));
    assert_eq!(m.workspace_dir.as_deref(), Some("/b"));
    assert_eq!(m.receipts_dir.as_deref(), Some("/c"));
}

#[test]
fn merge_valid_configs_still_valid() {
    let base = full_config();
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    validate_config(&m).unwrap();
}

#[test]
fn merge_introduces_invalid_log_level() {
    let base = full_config();
    let overlay = BackplaneConfig {
        log_level: Some("banana".into()),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert!(validate_config(&m).is_err());
}

#[test]
fn merge_introduces_bad_backend() {
    let base = full_config();
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("bad".into(), sidecar("", &[], None))]),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert!(validate_config(&m).is_err());
}

#[test]
fn merge_overlay_fixes_broken_base_backend() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar("", &[], None))]),
        ..full_config()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar("node", &[], None))]),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    validate_config(&m).unwrap();
}

#[test]
fn merged_config_accumulates_warnings() {
    let base = BackplaneConfig {
        default_backend: None,
        receipts_dir: None,
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("big".into(), sidecar("node", &[], Some(7_200)))]),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    let w = validate_config(&m).unwrap();
    assert!(w.len() >= 3, "expected >= 3 warnings: {w:?}");
}

// ###########################################################################
// 8. Edge cases
// ###########################################################################

#[test]
fn unknown_toml_fields_are_silently_ignored() {
    // serde default allows unknown fields — they are silently dropped
    let toml = r#"unknown_field = "hello""#;
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.default_backend.is_none());
}

#[test]
fn extra_fields_in_backend_silently_ignored() {
    let toml = r#"
        [backends.m]
        type = "mock"
        extra = "nope"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn very_long_backend_name_is_valid() {
    let mut cfg = full_config();
    cfg.backends
        .insert("a".repeat(10_000), BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

#[test]
fn very_long_command_is_valid() {
    let mut cfg = full_config();
    cfg.backends
        .insert("long".into(), sidecar(&"x".repeat(100_000), &[], None));
    validate_config(&cfg).unwrap();
}

#[test]
fn unicode_in_command() {
    let mut cfg = full_config();
    cfg.backends
        .insert("uni".into(), sidecar("nöde", &["日本語"], None));
    validate_config(&cfg).unwrap();
}

#[test]
fn special_chars_in_backend_name() {
    let mut cfg = full_config();
    cfg.backends
        .insert("my-backend_v2.0".into(), BackendEntry::Mock {});
    cfg.backends
        .insert("backend/slashes".into(), BackendEntry::Mock {});
    cfg.backends
        .insert("backend spaces".into(), BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

#[test]
fn special_chars_in_paths() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("/tmp/agent (copy)/work dir!/@#$".into()),
        receipts_dir: Some("/tmp/日本語/receipts".into()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn windows_style_paths() {
    let cfg = BackplaneConfig {
        workspace_dir: Some(r"C:\Users\agent\workspace".into()),
        receipts_dir: Some(r"D:\data\receipts".into()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn empty_workspace_dir_string_accepted() {
    let cfg = BackplaneConfig {
        workspace_dir: Some(String::new()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn empty_receipts_dir_string_accepted() {
    let cfg = BackplaneConfig {
        receipts_dir: Some(String::new()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn config_with_no_backends_is_valid() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::new(),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_command_with_leading_spaces_is_valid() {
    let mut cfg = full_config();
    cfg.backends
        .insert("sp".into(), sidecar("  node", &[], None));
    validate_config(&cfg).unwrap();
}

#[test]
fn very_long_log_level_is_invalid() {
    let cfg = BackplaneConfig {
        log_level: Some("x".repeat(1_000)),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn config_error_display_file_not_found() {
    let e = ConfigError::FileNotFound {
        path: "/missing.toml".into(),
    };
    assert!(e.to_string().contains("/missing.toml"));
}

#[test]
fn config_error_display_parse_error() {
    let e = ConfigError::ParseError {
        reason: "bad syntax".into(),
    };
    assert!(e.to_string().contains("bad syntax"));
}

#[test]
fn config_error_display_validation_error() {
    let e = ConfigError::ValidationError {
        reasons: vec!["reason1".into(), "reason2".into()],
    };
    let s = e.to_string();
    assert!(s.contains("reason1"));
    assert!(s.contains("reason2"));
}

#[test]
fn config_error_display_merge_conflict() {
    let e = ConfigError::MergeConflict {
        reason: "conflict!".into(),
    };
    assert!(e.to_string().contains("conflict!"));
}

// ###########################################################################
// Load from file tests
// ###########################################################################

#[test]
fn load_config_from_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "default_backend = \"mock\"\nlog_level = \"warn\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn load_config_missing_file() {
    let err = load_config(Some(Path::new("/nonexistent/backplane.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_config_none_returns_default() {
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn load_config_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.toml");
    std::fs::File::create(&path).unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn load_config_invalid_toml_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "[broken ===").unwrap();
    let err = load_config(Some(&path)).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn load_config_full_toml_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("full.toml");
    let content = r#"
default_backend = "sc"
workspace_dir = "/ws"
log_level = "debug"
receipts_dir = "/receipts"

[backends.m]
type = "mock"

[backends.sc]
type = "sidecar"
command = "node"
args = ["host.js"]
timeout_secs = 120
"#;
    std::fs::write(&path, content).unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("sc"));
    assert_eq!(cfg.backends.len(), 2);
}

// ###########################################################################
// TOML parsing edge cases
// ###########################################################################

#[test]
fn parse_toml_whitespace_only() {
    let cfg = parse_toml("   \n\n\t\t  ").unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn parse_toml_comment_only() {
    let cfg = parse_toml("# just a comment\n# another comment").unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn parse_toml_sidecar_with_empty_args_array() {
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

#[test]
fn parse_toml_backends_ordered_by_name() {
    let toml = r#"
        [backends.z_last]
        type = "mock"

        [backends.a_first]
        type = "mock"

        [backends.m_middle]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    let keys: Vec<&String> = cfg.backends.keys().collect();
    assert_eq!(keys, vec!["a_first", "m_middle", "z_last"]);
}

#[test]
fn json_schema_can_be_generated() {
    let schema = schemars::schema_for!(BackplaneConfig);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("BackplaneConfig"));
}

#[test]
fn parse_toml_boolean_for_string_field_is_error() {
    let err = parse_toml("default_backend = true").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_toml_array_for_string_field_is_error() {
    let err = parse_toml("log_level = [\"info\"]").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_toml_integer_for_args_is_error() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = 42
    "#;
    assert!(parse_toml(toml).is_err());
}

#[test]
fn parse_toml_negative_timeout_is_parse_error() {
    // TOML will parse negative as i64, but serde expects u64
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        timeout_secs = -1
    "#;
    assert!(parse_toml(toml).is_err());
}

#[test]
fn config_equality_same_content() {
    let a = full_config();
    let b = full_config();
    assert_eq!(a, b);
}

#[test]
fn config_inequality_different_default_backend() {
    let a = full_config();
    let mut b = full_config();
    b.default_backend = Some("different".into());
    assert_ne!(a, b);
}

#[test]
fn config_inequality_different_backends() {
    let a = full_config();
    let mut b = full_config();
    b.backends.insert("extra".into(), BackendEntry::Mock {});
    assert_ne!(a, b);
}
