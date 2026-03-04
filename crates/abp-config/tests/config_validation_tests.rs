// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for ConfigValidator::check, ConfigValidationResult, ConfigIssue,
//! IssueSeverity, ConfigMerger, ConfigDiff::diff, and ConfigChange.

use abp_config::validate::{
    ConfigChange, ConfigDiff, ConfigIssue, ConfigMerger, ConfigValidationResult, ConfigValidator,
    IssueSeverity,
};
use abp_config::{BackendEntry, BackplaneConfig};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fully-specified config that passes validation with no issues.
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
        ..Default::default()
    }
}

// ===========================================================================
// 1. ConfigValidator::check — valid config yields valid=true, no errors
// ===========================================================================

#[test]
fn check_valid_config_is_valid() {
    let result = ConfigValidator::check(&fully_valid_config());
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

// ===========================================================================
// 2. ConfigValidator::check — default config is valid but has warnings
// ===========================================================================

#[test]
fn check_default_config_valid_with_warnings() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    assert!(result.valid);
    assert!(!result.warnings.is_empty());
}

// ===========================================================================
// 3. Invalid log level produces an error with field path
// ===========================================================================

#[test]
fn check_invalid_log_level_error_has_field() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..fully_valid_config()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "log_level"));
}

// ===========================================================================
// 4. Empty sidecar command produces error with dotted path
// ===========================================================================

#[test]
fn check_empty_command_error_field_path() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: "  ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.field == "backends.bad.command")
    );
}

// ===========================================================================
// 5. Zero timeout produces error with field path
// ===========================================================================

#[test]
fn check_zero_timeout_error_field_path() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "z".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
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
            .any(|e| e.field == "backends.z.timeout_secs")
    );
}

// ===========================================================================
// 6. Timeout above max produces error
// ===========================================================================

#[test]
fn check_timeout_above_max_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_401),
        },
    );
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.field == "backends.big.timeout_secs")
    );
}

// ===========================================================================
// 7. Large timeout produces warning, not error
// ===========================================================================

#[test]
fn check_large_timeout_warning_not_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7_200),
        },
    );
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.field == "backends.big.timeout_secs"
                && w.severity == IssueSeverity::Warning)
    );
}

// ===========================================================================
// 8. Empty backend name produces error
// ===========================================================================

#[test]
fn check_empty_backend_name_error() {
    let mut cfg = fully_valid_config();
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

// ===========================================================================
// 9. Missing default_backend produces warning
// ===========================================================================

#[test]
fn check_missing_default_backend_warning() {
    let mut cfg = fully_valid_config();
    cfg.default_backend = None;
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| w.field == "default_backend"));
}

// ===========================================================================
// 10. Missing receipts_dir produces warning
// ===========================================================================

#[test]
fn check_missing_receipts_dir_warning() {
    let mut cfg = fully_valid_config();
    cfg.receipts_dir = None;
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| w.field == "receipts_dir"));
}

// ===========================================================================
// 11. Empty workspace_dir string produces warning
// ===========================================================================

#[test]
fn check_empty_workspace_dir_string_warning() {
    let mut cfg = fully_valid_config();
    cfg.workspace_dir = Some("".into());
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.field == "workspace_dir" && w.message.contains("empty"))
    );
}

// ===========================================================================
// 12. default_backend referencing unknown backend produces warning + suggestion
// ===========================================================================

#[test]
fn check_unknown_default_backend_warning_and_suggestion() {
    let mut cfg = fully_valid_config();
    cfg.default_backend = Some("nonexistent".into());
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.field == "default_backend" && w.message.contains("nonexistent"))
    );
    assert!(
        result
            .suggestions
            .iter()
            .any(|s| s.contains("Set default_backend"))
    );
}

// ===========================================================================
// 13. No backends produces suggestion
// ===========================================================================

#[test]
fn check_no_backends_suggestion() {
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
// 14. All errors have IssueSeverity::Error
// ===========================================================================

#[test]
fn check_all_errors_have_error_severity() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..fully_valid_config()
    };
    cfg.backends.insert(
        "broken".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let result = ConfigValidator::check(&cfg);
    assert!(
        result
            .errors
            .iter()
            .all(|e| e.severity == IssueSeverity::Error)
    );
}

// ===========================================================================
// 15. All warnings have IssueSeverity::Warning
// ===========================================================================

#[test]
fn check_all_warnings_have_warning_severity() {
    let result = ConfigValidator::check(&BackplaneConfig::default());
    assert!(
        result
            .warnings
            .iter()
            .all(|w| w.severity == IssueSeverity::Warning)
    );
}

// ===========================================================================
// 16. ConfigIssue Display includes field and message
// ===========================================================================

#[test]
fn config_issue_display() {
    let issue = ConfigIssue {
        field: "backends.sc.command".into(),
        message: "must not be empty".into(),
        severity: IssueSeverity::Error,
    };
    let s = issue.to_string();
    assert!(s.contains("[error]"));
    assert!(s.contains("backends.sc.command"));
    assert!(s.contains("must not be empty"));
}

// ===========================================================================
// 17. IssueSeverity Display
// ===========================================================================

#[test]
fn issue_severity_display() {
    assert_eq!(IssueSeverity::Error.to_string(), "error");
    assert_eq!(IssueSeverity::Warning.to_string(), "warning");
}

// ===========================================================================
// 18. IssueSeverity serialization roundtrip
// ===========================================================================

#[test]
fn issue_severity_serde_roundtrip() {
    let json = serde_json::to_string(&IssueSeverity::Error).unwrap();
    assert_eq!(json, "\"error\"");
    let back: IssueSeverity = serde_json::from_str(&json).unwrap();
    assert_eq!(back, IssueSeverity::Error);

    let json = serde_json::to_string(&IssueSeverity::Warning).unwrap();
    assert_eq!(json, "\"warning\"");
}

// ===========================================================================
// 19. ConfigValidationResult serialization roundtrip
// ===========================================================================

#[test]
fn config_validation_result_serde_roundtrip() {
    let result = ConfigValidator::check(&fully_valid_config());
    let json = serde_json::to_string(&result).unwrap();
    let back: ConfigValidationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.valid, result.valid);
    assert_eq!(back.errors.len(), result.errors.len());
    assert_eq!(back.warnings.len(), result.warnings.len());
}

// ===========================================================================
// 20. ConfigMerger::merge preserves base when overlay is default
// ===========================================================================

#[test]
fn merger_preserves_base() {
    let base = fully_valid_config();
    let merged = ConfigMerger::merge(&base, &BackplaneConfig::default());
    assert_eq!(merged.default_backend, base.default_backend);
    assert_eq!(merged.workspace_dir, base.workspace_dir);
    assert_eq!(merged.receipts_dir, base.receipts_dir);
    assert!(merged.backends.contains_key("mock"));
}

// ===========================================================================
// 21. ConfigMerger::merge — overlay wins on conflict
// ===========================================================================

#[test]
fn merger_overlay_wins() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = ConfigMerger::merge(&base, &overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
}

// ===========================================================================
// 22. ConfigMerger::merge — backends combined additively
// ===========================================================================

#[test]
fn merger_combines_backends() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = ConfigMerger::merge(&base, &overlay);
    assert!(merged.backends.contains_key("a"));
    assert!(merged.backends.contains_key("b"));
}

// ===========================================================================
// 23. ConfigMerger::merge — overlay backend replaces base on same key
// ===========================================================================

#[test]
fn merger_overlay_replaces_same_key() {
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
    let merged = ConfigMerger::merge(&base, &overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

// ===========================================================================
// 24. ConfigDiff::diff — identical configs produce no changes
// ===========================================================================

#[test]
fn diff_identical_no_changes() {
    let cfg = fully_valid_config();
    let changes = ConfigDiff::diff(&cfg, &cfg);
    assert!(changes.is_empty());
}

// ===========================================================================
// 25. ConfigDiff::diff — detects scalar field change
// ===========================================================================

#[test]
fn diff_detects_scalar_change() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.log_level = Some("debug".into());
    let changes = ConfigDiff::diff(&a, &b);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].field, "log_level");
    assert!(changes[0].old_value.contains("info"));
    assert!(changes[0].new_value.contains("debug"));
}

// ===========================================================================
// 26. ConfigDiff::diff — detects added backend
// ===========================================================================

#[test]
fn diff_detects_added_backend() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.backends.insert("new".into(), BackendEntry::Mock {});
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "backends.new"));
}

// ===========================================================================
// 27. ConfigDiff::diff — detects removed backend
// ===========================================================================

#[test]
fn diff_detects_removed_backend() {
    let a = fully_valid_config();
    let mut b = a.clone();
    b.backends.remove("mock");
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "backends.mock"));
}

// ===========================================================================
// 28. ConfigChange Display
// ===========================================================================

#[test]
fn config_change_display() {
    let c = ConfigChange {
        field: "log_level".into(),
        old_value: "\"info\"".into(),
        new_value: "\"debug\"".into(),
    };
    let s = c.to_string();
    assert!(s.contains("log_level"));
    assert!(s.contains("->"));
}

// ===========================================================================
// 29. ConfigChange serialization roundtrip
// ===========================================================================

#[test]
fn config_change_serde_roundtrip() {
    let change = ConfigChange {
        field: "log_level".into(),
        old_value: "\"info\"".into(),
        new_value: "\"debug\"".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let back: ConfigChange = serde_json::from_str(&json).unwrap();
    assert_eq!(back, change);
}

// ===========================================================================
// 30. check is idempotent
// ===========================================================================

#[test]
fn check_idempotent() {
    let cfg = fully_valid_config();
    let r1 = ConfigValidator::check(&cfg);
    let r2 = ConfigValidator::check(&cfg);
    assert_eq!(r1.valid, r2.valid);
    assert_eq!(r1.errors.len(), r2.errors.len());
    assert_eq!(r1.warnings.len(), r2.warnings.len());
    assert_eq!(r1.suggestions.len(), r2.suggestions.len());
}

// ===========================================================================
// 31. Multiple errors collected in single check
// ===========================================================================

#[test]
fn check_collects_multiple_errors() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..fully_valid_config()
    };
    cfg.backends.insert(
        "broken".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    // log_level + empty command + zero timeout = at least 3
    assert!(result.errors.len() >= 3);
}

// ===========================================================================
// 32. Merger result passes validation
// ===========================================================================

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

// ===========================================================================
// 33. Diff after merge shows what changed
// ===========================================================================

#[test]
fn diff_after_merge_shows_changes() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        log_level: Some("trace".into()),
        backends: BTreeMap::from([("new".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = ConfigMerger::merge(&base, &overlay);
    let changes = ConfigDiff::diff(&base, &merged);
    assert!(changes.iter().any(|c| c.field == "log_level"));
    assert!(changes.iter().any(|c| c.field == "backends.new"));
}

// ===========================================================================
// 34. ConfigIssue serde roundtrip
// ===========================================================================

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

// ===========================================================================
// 35. All valid log levels pass check
// ===========================================================================

#[test]
fn check_all_valid_log_levels() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..fully_valid_config()
        };
        let result = ConfigValidator::check(&cfg);
        assert!(result.valid, "log_level '{level}' should be valid");
    }
}
