// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep validation tests for `abp-config`.

use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, merge_configs, parse_toml,
    validate_config,
};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fully-specified config with no validation warnings.
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
        backends,
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
// 1. Valid configs pass validation with no warnings
// ===========================================================================

#[test]
fn fully_specified_config_has_no_warnings() {
    let warnings = validate_config(&fully_valid_config()).unwrap();
    assert!(warnings.is_empty(), "expected zero warnings: {warnings:?}");
}

#[test]
fn valid_config_all_log_levels() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..fully_valid_config()
        };
        validate_config(&cfg)
            .unwrap_or_else(|e| panic!("log_level '{level}' should be valid: {e}"));
    }
}

#[test]
fn valid_sidecar_at_boundary_timeout_1s() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "edge".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: vec![],
            timeout_secs: Some(1),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_sidecar_at_boundary_timeout_max() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "edge".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    // Should pass but may warn about large timeout.
    validate_config(&cfg).unwrap();
}

#[test]
fn valid_sidecar_no_timeout() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "no_to".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 2. Empty sidecar command is a hard error
// ===========================================================================

#[test]
fn empty_sidecar_command_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: String::new(),
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

// ===========================================================================
// 3. Whitespace-only sidecar command is a hard error
// ===========================================================================

#[test]
fn whitespace_only_command_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "ws".into(),
        BackendEntry::Sidecar {
            command: "   \t  ".into(),
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
fn tab_only_command_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "tab".into(),
        BackendEntry::Sidecar {
            command: "\t\t".into(),
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
fn newline_only_command_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "nl".into(),
        BackendEntry::Sidecar {
            command: "\n\n".into(),
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

// ===========================================================================
// 4. Out-of-range timeout is a hard error
// ===========================================================================

#[test]
fn timeout_exceeds_max_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_401),
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

#[test]
fn timeout_way_over_max_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "huge".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(u64::MAX),
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

// ===========================================================================
// 5. Zero timeout is a hard error
// ===========================================================================

#[test]
fn zero_timeout_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "zero".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

// ===========================================================================
// 6. Invalid log levels generate errors
// ===========================================================================

#[test]
fn invalid_log_level_verbose() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn invalid_log_level_uppercase() {
    let cfg = BackplaneConfig {
        log_level: Some("INFO".into()),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn invalid_log_level_empty_string() {
    let cfg = BackplaneConfig {
        log_level: Some(String::new()),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn invalid_log_level_numeric_string() {
    let cfg = BackplaneConfig {
        log_level: Some("0".into()),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn none_log_level_is_valid() {
    let cfg = BackplaneConfig {
        log_level: None,
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 7. Multiple validation errors can be collected
// ===========================================================================

#[test]
fn multiple_errors_collected() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bad_level".into()),
        default_backend: Some("x".into()),
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    cfg.backends.insert(
        "a".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    cfg.backends.insert(
        "b".into(),
        BackendEntry::Sidecar {
            command: "  ".into(),
            args: vec![],
            timeout_secs: Some(999_999),
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    // log_level error + two command errors + two timeout errors = at least 5
    assert!(
        reasons.len() >= 5,
        "expected >= 5 errors, got {}: {reasons:?}",
        reasons.len()
    );
}

#[test]
fn empty_command_and_zero_timeout_both_reported() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "broken".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("command must not be empty"))
    );
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

#[test]
fn empty_backend_name_counted_as_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
}

// ===========================================================================
// 8. Validation warnings for non-critical issues
// ===========================================================================

#[test]
fn missing_default_backend_warns() {
    let cfg = BackplaneConfig {
        default_backend: None,
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn missing_receipts_dir_warns() {
    let cfg = BackplaneConfig {
        default_backend: Some("x".into()),
        receipts_dir: None,
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
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
    let warnings = validate_config(&cfg).unwrap();
    let missing_count = warnings
        .iter()
        .filter(|w| matches!(w, ConfigWarning::MissingOptionalField { .. }))
        .count();
    assert_eq!(missing_count, 2);
}

#[test]
fn large_timeout_warning_threshold() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_601), // just above 3600
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, secs } if backend == "big" && *secs == 3_601
    )));
}

#[test]
fn exactly_at_threshold_no_large_timeout_warning() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "exact".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_600), // exactly at threshold
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(!warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "exact"
    )));
}

#[test]
fn just_below_threshold_no_large_timeout_warning() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "below".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_599),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(!warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "below"
    )));
}

// ===========================================================================
// 9. Backend-specific validation (mock vs sidecar)
// ===========================================================================

#[test]
fn mock_backend_always_valid() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("m1".into(), BackendEntry::Mock {});
    cfg.backends.insert("m2".into(), BackendEntry::Mock {});
    cfg.backends.insert("m3".into(), BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_with_valid_command_passes() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "ok".into(),
        BackendEntry::Sidecar {
            command: "/usr/bin/python3".into(),
            args: vec!["--flag".into(), "arg".into()],
            timeout_secs: Some(60),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_empty_args_is_fine() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "ok".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn mix_of_mock_and_sidecar_valid() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("m".into(), BackendEntry::Mock {});
    cfg.backends.insert(
        "s1".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(60),
        },
    );
    cfg.backends.insert(
        "s2".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: vec!["host.py".into()],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn one_bad_sidecar_among_good_backends_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("m".into(), BackendEntry::Mock {});
    cfg.backends.insert(
        "good".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(60),
        },
    );
    cfg.backends.insert(
        "broken".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("broken")));
    assert_eq!(reasons.len(), 1, "only the broken backend should error");
}

// ===========================================================================
// 10. Path validation for workspace_dir and receipts_dir
// ===========================================================================

#[test]
fn workspace_dir_accepts_any_string() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("/some/path".into()),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn receipts_dir_accepts_any_string() {
    let cfg = BackplaneConfig {
        receipts_dir: Some("./relative/path".into()),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn windows_style_paths_accepted() {
    let cfg = BackplaneConfig {
        workspace_dir: Some(r"C:\Users\agent\workspace".into()),
        receipts_dir: Some(r"D:\data\receipts".into()),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn paths_with_spaces_accepted() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("/path with spaces/ws".into()),
        receipts_dir: Some("/path with spaces/receipts".into()),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn empty_workspace_dir_accepted() {
    // Empty string is technically allowed — the validator doesn't enforce non-empty paths.
    let cfg = BackplaneConfig {
        workspace_dir: Some(String::new()),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 11. Environment variable interaction with validation
// ===========================================================================

#[test]
fn env_override_log_level_then_validate() {
    // Simulates what happens when env sets an invalid log level.
    let mut cfg = fully_valid_config();
    // Pretend env set log_level to something invalid.
    cfg.log_level = Some("INVALID_FROM_ENV".into());
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn env_override_default_backend_removes_warning() {
    let mut cfg = BackplaneConfig {
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    // Without default_backend → warning.
    let w1 = validate_config(&cfg).unwrap();
    assert!(w1.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
    // After "env override" sets it.
    cfg.default_backend = Some("mock".into());
    let w2 = validate_config(&cfg).unwrap();
    assert!(!w2.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

// ===========================================================================
// 12. Merged config validation
// ===========================================================================

#[test]
fn merged_valid_configs_still_valid() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    validate_config(&merged).unwrap();
}

#[test]
fn merge_introduces_invalid_log_level() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        log_level: Some("banana".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    let reasons = validation_reasons(validate_config(&merged).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn merge_introduces_bad_backend() {
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
    let reasons = validation_reasons(validate_config(&merged).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("command must not be empty"))
    );
}

#[test]
fn merge_overlay_fixes_base_backend() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..fully_valid_config()
    };
    // Overlay replaces the broken backend with a valid one.
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    validate_config(&merged).unwrap();
}

#[test]
fn merged_config_accumulates_warnings() {
    let base = BackplaneConfig {
        default_backend: None,
        receipts_dir: None,
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "big".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(7_200),
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    let warnings = validate_config(&merged).unwrap();
    // At least: missing default_backend + missing receipts_dir + large timeout
    assert!(warnings.len() >= 3, "expected >= 3 warnings: {warnings:?}");
}

// ===========================================================================
// 13. Edge cases: very long strings, special characters, empty names
// ===========================================================================

#[test]
fn very_long_backend_name() {
    let mut cfg = fully_valid_config();
    let name = "a".repeat(10_000);
    cfg.backends.insert(name, BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

#[test]
fn very_long_command() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "long".into(),
        BackendEntry::Sidecar {
            command: "x".repeat(100_000),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn special_characters_in_backend_name() {
    let mut cfg = fully_valid_config();
    cfg.backends
        .insert("my-backend_v2.0".into(), BackendEntry::Mock {});
    cfg.backends
        .insert("backend/with/slashes".into(), BackendEntry::Mock {});
    cfg.backends
        .insert("backend with spaces".into(), BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

#[test]
fn unicode_in_command() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "uni".into(),
        BackendEntry::Sidecar {
            command: "nöde".into(),
            args: vec!["—flag".into(), "日本語".into()],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn empty_backend_name_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
}

#[test]
fn special_characters_in_paths() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("/tmp/agent (copy)/work dir!/@#$".into()),
        receipts_dir: Some("/tmp/日本語/receipts".into()),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn very_long_log_level_is_invalid() {
    let cfg = BackplaneConfig {
        log_level: Some("x".repeat(1_000)),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn many_backends_all_valid() {
    let mut cfg = fully_valid_config();
    for i in 0..100 {
        cfg.backends
            .insert(format!("mock_{i}"), BackendEntry::Mock {});
    }
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 14. Schema conformance after validation
// ===========================================================================

#[test]
fn valid_config_serializes_to_json() {
    let cfg = fully_valid_config();
    validate_config(&cfg).unwrap();
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    assert!(json.contains("\"default_backend\""));
    assert!(json.contains("\"mock\""));
}

#[test]
fn valid_config_roundtrips_via_json() {
    let cfg = fully_valid_config();
    validate_config(&cfg).unwrap();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn default_config_roundtrips_via_json() {
    let cfg = BackplaneConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn toml_roundtrip_preserves_validity() {
    let cfg = fully_valid_config();
    validate_config(&cfg).unwrap();
    let toml_str = toml::to_string(&cfg).unwrap();
    let back = parse_toml(&toml_str).unwrap();
    let warnings = validate_config(&back).unwrap();
    assert!(warnings.is_empty());
}

#[test]
fn json_schema_can_be_generated() {
    let schema = schemars::schema_for!(BackplaneConfig);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("BackplaneConfig"));
}

// ===========================================================================
// 15. Validation idempotency (validate twice = same result)
// ===========================================================================

#[test]
fn idempotent_valid_config() {
    let cfg = fully_valid_config();
    let w1 = validate_config(&cfg).unwrap();
    let w2 = validate_config(&cfg).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn idempotent_default_config() {
    let cfg = BackplaneConfig::default();
    let w1 = validate_config(&cfg).unwrap();
    let w2 = validate_config(&cfg).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn idempotent_config_with_warnings() {
    let mut cfg = fully_valid_config();
    cfg.default_backend = None;
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7_200),
        },
    );
    let w1 = validate_config(&cfg).unwrap();
    let w2 = validate_config(&cfg).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn idempotent_invalid_config() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..fully_valid_config()
    };
    let e1 = validate_config(&cfg).unwrap_err();
    let e2 = validate_config(&cfg).unwrap_err();
    let r1 = validation_reasons(e1);
    let r2 = validation_reasons(e2);
    assert_eq!(r1, r2);
}

#[test]
fn idempotent_multiple_errors() {
    let mut cfg = BackplaneConfig {
        log_level: Some("nope".into()),
        ..fully_valid_config()
    };
    cfg.backends.insert(
        "".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let r1 = validation_reasons(validate_config(&cfg).unwrap_err());
    let r2 = validation_reasons(validate_config(&cfg).unwrap_err());
    assert_eq!(r1, r2);
}

// ===========================================================================
// Additional edge-case tests
// ===========================================================================

#[test]
fn config_with_no_backends_is_valid() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::new(),
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_command_with_leading_spaces_is_valid() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "spaces".into(),
        BackendEntry::Sidecar {
            command: "  node".into(), // has leading whitespace but non-empty after trim
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn multiple_large_timeouts_produce_multiple_warnings() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "big1".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7_200),
        },
    );
    cfg.backends.insert(
        "big2".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: vec![],
            timeout_secs: Some(43_200),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    let lt_count = warnings
        .iter()
        .filter(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
        .count();
    assert_eq!(lt_count, 2);
}

#[test]
fn parse_toml_sidecar_missing_command_fails() {
    let toml = r#"
        [backends.bad]
        type = "sidecar"
        args = []
    "#;
    // TOML parse should fail because `command` is required.
    assert!(parse_toml(toml).is_err());
}

#[test]
fn config_warning_display_for_missing_optional() {
    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "receipts will not be persisted to disk".into(),
    };
    let s = w.to_string();
    assert!(s.contains("receipts_dir"));
    assert!(s.contains("persisted"));
}

#[test]
fn validation_error_display_contains_all_reasons() {
    let err = ConfigError::ValidationError {
        reasons: vec!["reason one".into(), "reason two".into()],
    };
    let s = err.to_string();
    assert!(s.contains("reason one"));
    assert!(s.contains("reason two"));
}
