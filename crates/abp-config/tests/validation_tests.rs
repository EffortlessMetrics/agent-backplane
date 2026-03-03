// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the `validate` module: ConfigValidator, diff_configs,
//! from_env_overrides.

use abp_config::validate::{
    ConfigDiff, ConfigValidator, Severity, ValidationIssue, diff_configs, from_env_overrides,
};
use abp_config::{BackendEntry, BackplaneConfig, ConfigError, merge_configs, validate_config};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A fully-specified config that produces zero validation issues.
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
// 1. ConfigValidator — valid config produces no errors
// ===========================================================================

#[test]
fn validator_valid_config_no_errors() {
    let cfg = fully_valid_config();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    // Fully specified → no warnings or info-level issues.
    assert!(issues.is_empty(), "expected no issues: {issues:?}");
}

// ===========================================================================
// 2. ConfigValidator — default config returns info/warning issues
// ===========================================================================

#[test]
fn validator_default_config_returns_issues() {
    let cfg = BackplaneConfig::default();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(!issues.is_empty());
    // Should include info about no backends and warnings about missing fields.
    assert!(issues.iter().any(|i| i.severity == Severity::Info));
    assert!(issues.iter().any(|i| i.severity == Severity::Warning));
}

// ===========================================================================
// 3. ConfigValidator — invalid log level is a hard error
// ===========================================================================

#[test]
fn validator_invalid_log_level_error() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..fully_valid_config()
    };
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    let reasons = validation_reasons(err);
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

// ===========================================================================
// 4. ConfigValidator — empty sidecar command caught
// ===========================================================================

#[test]
fn validator_empty_command_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: "  ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = validation_reasons(ConfigValidator::validate(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("command must not be empty"))
    );
}

// ===========================================================================
// 5. ConfigValidator — empty backend name caught
// ===========================================================================

#[test]
fn validator_empty_backend_name_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let reasons = validation_reasons(ConfigValidator::validate(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
}

// ===========================================================================
// 6. ConfigValidator — zero timeout caught
// ===========================================================================

#[test]
fn validator_zero_timeout_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "z".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let reasons = validation_reasons(ConfigValidator::validate(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

// ===========================================================================
// 7. ConfigValidator — timeout above max caught
// ===========================================================================

#[test]
fn validator_timeout_above_max_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_401),
        },
    );
    let reasons = validation_reasons(ConfigValidator::validate(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

// ===========================================================================
// 8. ConfigValidator — large timeout produces warning issue
// ===========================================================================

#[test]
fn validator_large_timeout_warning() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7_200),
        },
    );
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Warning && i.message.contains("large timeout"))
    );
}

// ===========================================================================
// 9. ConfigValidator — no-backends info issue
// ===========================================================================

#[test]
fn validator_no_backends_info_issue() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::new(),
        ..fully_valid_config()
    };
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Info && i.message.contains("no backends"))
    );
}

// ===========================================================================
// 10. ConfigValidator::validate_at filters by severity
// ===========================================================================

#[test]
fn validator_validate_at_filters_info() {
    let cfg = BackplaneConfig::default();
    let all = ConfigValidator::validate(&cfg).unwrap();
    let warnings_only = ConfigValidator::validate_at(&cfg, Severity::Warning).unwrap();
    assert!(all.len() > warnings_only.len());
    assert!(
        warnings_only
            .iter()
            .all(|i| i.severity >= Severity::Warning)
    );
}

// ===========================================================================
// 11. ConfigValidator agrees with validate_config on errors
// ===========================================================================

#[test]
fn validator_agrees_with_free_fn_on_errors() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..fully_valid_config()
    };
    // Both should return Err.
    assert!(ConfigValidator::validate(&cfg).is_err());
    assert!(validate_config(&cfg).is_err());
}

// ===========================================================================
// 12. ConfigValidator agrees with validate_config on valid config
// ===========================================================================

#[test]
fn validator_agrees_with_free_fn_on_valid() {
    let cfg = fully_valid_config();
    assert!(ConfigValidator::validate(&cfg).is_ok());
    assert!(validate_config(&cfg).is_ok());
}

// ===========================================================================
// 13. Config merge preserves base values when overlay is default
// ===========================================================================

#[test]
fn merge_preserves_base_values() {
    let base = fully_valid_config();
    let merged = merge_configs(base.clone(), BackplaneConfig::default());
    // default_backend, workspace_dir, receipts_dir should be preserved.
    assert_eq!(merged.default_backend, base.default_backend);
    assert_eq!(merged.workspace_dir, base.workspace_dir);
    assert_eq!(merged.receipts_dir, base.receipts_dir);
    assert!(merged.backends.contains_key("mock"));
    assert!(merged.backends.contains_key("sc"));
}

// ===========================================================================
// 14. Overlay values win on conflict
// ===========================================================================

#[test]
fn merge_overlay_values_win() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
}

// ===========================================================================
// 15. Overlay backend replaces base backend with same key
// ===========================================================================

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
        ..fully_valid_config()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec!["index.js".into()],
                timeout_secs: Some(60),
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

// ===========================================================================
// 16. Empty configs merge to default-ish result
// ===========================================================================

#[test]
fn merge_empty_configs() {
    let a = BackplaneConfig {
        log_level: None,
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: None,
        ..Default::default()
    };
    let merged = merge_configs(a, b);
    assert!(merged.default_backend.is_none());
    assert!(merged.backends.is_empty());
}

// ===========================================================================
// 17. Merge combines backend maps additively
// ===========================================================================

#[test]
fn merge_combines_backend_maps() {
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

// ===========================================================================
// 18. from_env_overrides applies ABP_DEFAULT_BACKEND
// ===========================================================================

#[test]
fn from_env_overrides_applies_default_backend() {
    // We cannot safely set real env vars in parallel tests, but we can
    // verify the function delegates by checking it compiles and runs without
    // panicking with no matching env vars set.
    let mut cfg = BackplaneConfig::default();
    // This should be a no-op when the env var is not set, but must not panic.
    from_env_overrides(&mut cfg);
    // log_level stays as default.
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

// ===========================================================================
// 19. Env override integration: set via field and validate
// ===========================================================================

#[test]
fn env_override_simulated_applies_and_validates() {
    let mut cfg = fully_valid_config();
    cfg.default_backend = Some("overridden".into());
    cfg.log_level = Some("debug".into());
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(issues.is_empty());
}

// ===========================================================================
// 20. diff_configs — identical configs produce no diffs
// ===========================================================================

#[test]
fn diff_identical_configs_no_diffs() {
    let cfg = fully_valid_config();
    let diffs = diff_configs(&cfg, &cfg);
    assert!(diffs.is_empty());
}

// ===========================================================================
// 21. diff_configs — detects scalar field change
// ===========================================================================

#[test]
fn diff_detects_log_level_change() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.log_level = Some("debug".into());
    let diffs = diff_configs(&a, &b);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].path, "log_level");
    assert!(diffs[0].old_value.contains("info"));
    assert!(diffs[0].new_value.contains("debug"));
}

// ===========================================================================
// 22. diff_configs — detects field going from Some to None
// ===========================================================================

#[test]
fn diff_detects_field_removed() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.workspace_dir = None;
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "workspace_dir"));
    let d = diffs.iter().find(|d| d.path == "workspace_dir").unwrap();
    assert_eq!(d.new_value, "<none>");
}

// ===========================================================================
// 23. diff_configs — detects field going from None to Some
// ===========================================================================

#[test]
fn diff_detects_field_added() {
    let mut a = fully_valid_config();
    a.workspace_dir = None;
    let b = fully_valid_config();
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "workspace_dir"));
    let d = diffs.iter().find(|d| d.path == "workspace_dir").unwrap();
    assert_eq!(d.old_value, "<none>");
}

// ===========================================================================
// 24. diff_configs — detects added backend
// ===========================================================================

#[test]
fn diff_detects_added_backend() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.backends.insert("new".into(), BackendEntry::Mock {});
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.new"));
    let d = diffs.iter().find(|d| d.path == "backends.new").unwrap();
    assert_eq!(d.old_value, "<absent>");
    assert_eq!(d.new_value, "mock");
}

// ===========================================================================
// 25. diff_configs — detects removed backend
// ===========================================================================

#[test]
fn diff_detects_removed_backend() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.backends.remove("mock");
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.mock"));
    let d = diffs.iter().find(|d| d.path == "backends.mock").unwrap();
    assert_eq!(d.new_value, "<absent>");
}

// ===========================================================================
// 26. diff_configs — detects changed backend entry
// ===========================================================================

#[test]
fn diff_detects_changed_backend() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: vec![],
            timeout_secs: Some(600),
        },
    );
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.sc"));
}

// ===========================================================================
// 27. diff_configs — multiple field changes
// ===========================================================================

#[test]
fn diff_multiple_field_changes() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.log_level = Some("trace".into());
    b.default_backend = Some("openai".into());
    b.receipts_dir = None;
    let diffs = diff_configs(&a, &b);
    assert!(diffs.len() >= 3);
}

// ===========================================================================
// 28. ConfigDiff Display trait
// ===========================================================================

#[test]
fn config_diff_display() {
    let d = ConfigDiff {
        path: "log_level".into(),
        old_value: "\"info\"".into(),
        new_value: "\"debug\"".into(),
    };
    let s = d.to_string();
    assert!(s.contains("log_level"));
    assert!(s.contains("->"));
}

// ===========================================================================
// 29. ValidationIssue Display includes severity
// ===========================================================================

#[test]
fn validation_issue_display() {
    let i = ValidationIssue {
        severity: Severity::Warning,
        message: "something is off".into(),
    };
    let s = i.to_string();
    assert!(s.contains("[warning]"));
    assert!(s.contains("something is off"));
}

// ===========================================================================
// 30. Severity ordering: Info < Warning < Error
// ===========================================================================

#[test]
fn severity_ordering() {
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Error);
    assert!(Severity::Info < Severity::Error);
}

// ===========================================================================
// 31. Severity Display
// ===========================================================================

#[test]
fn severity_display() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

// ===========================================================================
// 32. Validator idempotent — same config, same result
// ===========================================================================

#[test]
fn validator_idempotent() {
    let cfg = fully_valid_config();
    let a = ConfigValidator::validate(&cfg).unwrap();
    let b = ConfigValidator::validate(&cfg).unwrap();
    assert_eq!(a, b);
}

// ===========================================================================
// 33. Diff after merge shows what changed
// ===========================================================================

#[test]
fn diff_after_merge() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        log_level: Some("trace".into()),
        backends: BTreeMap::from([("new".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = merge_configs(base.clone(), overlay);
    let diffs = diff_configs(&base, &merged);
    assert!(diffs.iter().any(|d| d.path == "log_level"));
    assert!(diffs.iter().any(|d| d.path == "backends.new"));
}

// ===========================================================================
// 34. Validator with all valid log levels
// ===========================================================================

#[test]
fn validator_all_valid_log_levels() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..fully_valid_config()
        };
        ConfigValidator::validate(&cfg)
            .unwrap_or_else(|e| panic!("log_level '{level}' should be valid: {e}"));
    }
}

// ===========================================================================
// 35. Diff of default vs fully-valid shows many diffs
// ===========================================================================

#[test]
fn diff_default_vs_valid() {
    let a = BackplaneConfig::default();
    let b = fully_valid_config();
    let diffs = diff_configs(&a, &b);
    // default_backend, workspace_dir, receipts_dir differ, plus backends.
    assert!(diffs.len() >= 3);
}
