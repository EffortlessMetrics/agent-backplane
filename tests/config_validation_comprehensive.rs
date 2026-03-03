// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive config validation tests covering TOML parsing, validation
//! rules, config merging, error messages, and WorkOrder config mapping.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, load_config, merge_configs,
    parse_toml, validate_config,
};
use abp_core::config::{ConfigDefaults, ConfigValidator, WarningSeverity};
use abp_core::{ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrderBuilder, WorkspaceMode};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fully_valid_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/receipts".into()),
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

fn validation_reasons(err: ConfigError) -> Vec<String> {
    match err {
        ConfigError::ValidationError { reasons } => reasons,
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===========================================================================
// 1. TOML parsing (15 tests)
// ===========================================================================

#[test]
fn toml_empty_string_parses_to_default() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_all_scalar_fields() {
    let toml = r#"
        default_backend = "mock"
        workspace_dir = "/ws"
        log_level = "debug"
        receipts_dir = "/receipts"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/ws"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/receipts"));
}

#[test]
fn toml_nested_backend_table_mock() {
    let toml = "[backends.test]\ntype = \"mock\"";
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["test"], BackendEntry::Mock {}));
}

#[test]
fn toml_nested_backend_table_sidecar_all_fields() {
    let toml = r#"
        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["--flag", "host.js"]
        timeout_secs = 600
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["openai"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["--flag", "host.js"]);
            assert_eq!(*timeout_secs, Some(600));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_multiple_backends() {
    let toml = r#"
        [backends.mock]
        type = "mock"

        [backends.sc1]
        type = "sidecar"
        command = "node"

        [backends.sc2]
        type = "sidecar"
        command = "python3"
        args = ["host.py"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
}

#[test]
fn toml_sidecar_empty_args_array() {
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
fn toml_sidecar_omitted_args_defaults_empty() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_sidecar_omitted_timeout_defaults_none() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert!(timeout_secs.is_none()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_invalid_syntax_gives_parse_error() {
    let err = parse_toml("this is [not valid =").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_wrong_type_log_level_integer() {
    let err = parse_toml("log_level = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_wrong_type_backends_not_table() {
    let err = parse_toml("backends = 123").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_unknown_backend_type_gives_parse_error() {
    let toml = "[backends.bad]\ntype = \"unknown_type\"";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_sidecar_missing_command_gives_parse_error() {
    let toml = "[backends.bad]\ntype = \"sidecar\"\nargs = []";
    assert!(parse_toml(toml).is_err());
}

#[test]
fn toml_roundtrip_serialization() {
    let cfg = fully_valid_config();
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn toml_example_file_structure() {
    // Validates a config structured like backplane.example.toml
    let toml = r#"
        [backends.mock]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["path/to/openai-sidecar.js"]

        [backends.anthropic]
        type = "sidecar"
        command = "python3"
        args = ["path/to/anthropic-sidecar.py"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
}

// ===========================================================================
// 2. Validation rules (15 tests)
// ===========================================================================

#[test]
fn validation_fully_specified_config_no_warnings() {
    let warnings = validate_config(&fully_valid_config()).unwrap();
    assert!(warnings.is_empty());
}

#[test]
fn validation_default_config_has_advisory_warnings() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(!warnings.is_empty());
}

#[test]
fn validation_all_valid_log_levels() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..fully_valid_config()
        };
        validate_config(&cfg).unwrap_or_else(|e| panic!("'{level}' should be valid: {e}"));
    }
}

#[test]
fn validation_invalid_log_level_uppercase() {
    let cfg = BackplaneConfig {
        log_level: Some("DEBUG".into()),
        ..fully_valid_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn validation_invalid_log_level_arbitrary() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..fully_valid_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn validation_none_log_level_is_valid() {
    let cfg = BackplaneConfig {
        log_level: None,
        ..fully_valid_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_empty_sidecar_command() {
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

#[test]
fn validation_whitespace_only_command() {
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
fn validation_zero_timeout_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "z".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

#[test]
fn validation_timeout_exceeds_max() {
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
fn validation_timeout_boundary_1s_valid() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "edge".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(1),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_timeout_boundary_max_valid() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "edge".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn validation_empty_backend_name() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
}

#[test]
fn validation_multiple_errors_collected() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bad".into()),
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
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    // log_level + empty command + zero timeout = at least 3
    assert!(
        reasons.len() >= 3,
        "got {} errors: {reasons:?}",
        reasons.len()
    );
}

#[test]
fn validation_mock_backend_always_valid() {
    let mut cfg = fully_valid_config();
    for i in 0..10 {
        cfg.backends.insert(format!("m{i}"), BackendEntry::Mock {});
    }
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 3. Config merging (10 tests)
// ===========================================================================

#[test]
fn merge_overlay_overrides_default_backend() {
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
        workspace_dir: Some("/ws".into()),
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/ws"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/r"));
}

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

#[test]
fn merge_overlay_backend_wins_on_collision() {
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
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_overlay_log_level_takes_precedence() {
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
        receipts_dir: Some("/old_receipts".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        receipts_dir: Some("/new_receipts".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.receipts_dir.as_deref(), Some("/new_receipts"));
}

#[test]
fn merge_env_override_after_merge() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig::default();
    let mut merged = merge_configs(base, overlay);
    // Simulate env override
    merged.log_level = Some("trace".into());
    let warnings = validate_config(&merged).unwrap();
    assert!(warnings.is_empty());
}

#[test]
fn merge_merged_config_still_valid() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    validate_config(&merged).unwrap();
}

#[test]
fn merge_overlay_introduces_bad_backend_detected() {
    let base = fully_valid_config();
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "broken".into(),
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

// ===========================================================================
// 4. Error messages (10 tests)
// ===========================================================================

#[test]
fn error_file_not_found_display() {
    let e = ConfigError::FileNotFound {
        path: "/nonexistent/backplane.toml".into(),
    };
    let s = e.to_string();
    assert!(s.contains("not found"));
    assert!(s.contains("/nonexistent/backplane.toml"));
}

#[test]
fn error_parse_error_display() {
    let e = ConfigError::ParseError {
        reason: "expected `=`, found newline".into(),
    };
    let s = e.to_string();
    assert!(s.contains("parse"));
    assert!(s.contains("expected `=`, found newline"));
}

#[test]
fn error_validation_error_display_contains_all_reasons() {
    let e = ConfigError::ValidationError {
        reasons: vec!["invalid log_level 'foo'".into(), "empty command".into()],
    };
    let s = e.to_string();
    assert!(s.contains("invalid log_level"));
    assert!(s.contains("empty command"));
}

#[test]
fn error_merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "conflicting backend types".into(),
    };
    let s = e.to_string();
    assert!(s.contains("merge conflict"));
    assert!(s.contains("conflicting backend types"));
}

#[test]
fn warning_deprecated_field_with_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: Some("new_field".into()),
    };
    let s = w.to_string();
    assert!(s.contains("deprecated"));
    assert!(s.contains("old_field"));
    assert!(s.contains("new_field"));
}

#[test]
fn warning_deprecated_field_without_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "legacy".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("deprecated"));
    assert!(s.contains("legacy"));
}

#[test]
fn warning_missing_optional_field_display() {
    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "receipts will not be persisted to disk".into(),
    };
    let s = w.to_string();
    assert!(s.contains("receipts_dir"));
    assert!(s.contains("persisted"));
}

#[test]
fn warning_large_timeout_display() {
    let w = ConfigWarning::LargeTimeout {
        backend: "slow_backend".into(),
        secs: 7200,
    };
    let s = w.to_string();
    assert!(s.contains("slow_backend"));
    assert!(s.contains("7200"));
}

#[test]
fn error_load_missing_file_is_file_not_found() {
    let err = load_config(Some(Path::new("/nonexistent/backplane.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn error_validation_reasons_reference_backend_name() {
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
    assert!(reasons.iter().any(|r| r.contains("my_broken_backend")));
}

// ===========================================================================
// 5. WorkOrder config (15 tests)
// ===========================================================================

#[test]
fn workorder_empty_task_is_error() {
    let wo = WorkOrderBuilder::new("").build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "task"));
    assert!(
        warnings
            .iter()
            .any(|w| w.severity == WarningSeverity::Error)
    );
}

#[test]
fn workorder_whitespace_task_is_error() {
    let wo = WorkOrderBuilder::new("   \t  ").build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "task"));
}

#[test]
fn workorder_valid_task_no_task_warning() {
    let wo = WorkOrderBuilder::new("Refactor auth module").build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(!warnings.iter().any(|w| w.field == "task"));
}

#[test]
fn workorder_zero_max_turns_is_error() {
    let wo = WorkOrderBuilder::new("test").max_turns(0).build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "config.max_turns" && w.severity == WarningSeverity::Error)
    );
}

#[test]
fn workorder_valid_max_turns_no_warning() {
    let wo = WorkOrderBuilder::new("test").max_turns(10).build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(!warnings.iter().any(|w| w.field == "config.max_turns"));
}

#[test]
fn workorder_zero_budget_is_error() {
    let wo = WorkOrderBuilder::new("test").max_budget_usd(0.0).build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "config.max_budget_usd" && w.severity == WarningSeverity::Error)
    );
}

#[test]
fn workorder_negative_budget_is_error() {
    let wo = WorkOrderBuilder::new("test").max_budget_usd(-5.0).build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn workorder_valid_budget_no_warning() {
    let wo = WorkOrderBuilder::new("test").max_budget_usd(1.5).build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(!warnings.iter().any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn workorder_empty_model_name_is_error() {
    let wo = WorkOrderBuilder::new("test").model("  ").build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "config.model" && w.severity == WarningSeverity::Error)
    );
}

#[test]
fn workorder_valid_model_name() {
    let wo = WorkOrderBuilder::new("test").model("gpt-4").build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(!warnings.iter().any(|w| w.field == "config.model"));
}

#[test]
fn workorder_duplicate_tools_in_allowlist() {
    let wo = WorkOrderBuilder::new("test")
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into(), "read".into()],
            ..Default::default()
        })
        .build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "policy.allowed_tools" && w.message.contains("Duplicate"))
    );
}

#[test]
fn workorder_empty_glob_in_deny_read() {
    let wo = WorkOrderBuilder::new("test")
        .policy(PolicyProfile {
            deny_read: vec!["*.log".into(), "  ".into()],
            ..Default::default()
        })
        .build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "policy.deny_read" && w.severity == WarningSeverity::Error)
    );
}

#[test]
fn workorder_vendor_config_empty_key_is_error() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("  ".into(), serde_json::json!("value"));
    let wo = WorkOrderBuilder::new("test").config(config).build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "config.vendor" && w.severity == WarningSeverity::Error)
    );
}

#[test]
fn workorder_defaults_applied() {
    let mut wo = WorkOrderBuilder::new("test").build();
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.config.model.is_none());

    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(
        wo.config.max_turns,
        Some(ConfigDefaults::default_max_turns())
    );
    assert_eq!(
        wo.config.max_budget_usd,
        Some(ConfigDefaults::default_max_budget())
    );
    assert_eq!(
        wo.config.model.as_deref(),
        Some(ConfigDefaults::default_model())
    );
}

#[test]
fn workorder_vendor_config_with_abp_nested() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({
            "mode": "passthrough",
            "request": {"key": "value"}
        }),
    );
    let wo = WorkOrderBuilder::new("test").config(config).build();

    // Should parse without issues
    let abp_val = &wo.config.vendor["abp"];
    assert_eq!(abp_val["mode"], "passthrough");
    assert_eq!(abp_val["request"]["key"], "value");
}

// ===========================================================================
// Additional coverage tests (to reach 65+)
// ===========================================================================

#[test]
fn toml_json_roundtrip() {
    let cfg = fully_valid_config();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn validation_large_timeout_produces_warning() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "slow".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7200),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, secs } if backend == "slow" && *secs == 7200
    )));
}

#[test]
fn validation_exactly_at_threshold_no_warning() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "exact".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_600),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(!warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "exact"
    )));
}

#[test]
fn validation_just_above_threshold_warns() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert(
        "above".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_601),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "above"
    )));
}

#[test]
fn validation_missing_default_backend_warns() {
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
fn validation_missing_receipts_dir_warns() {
    let cfg = BackplaneConfig {
        default_backend: Some("m".into()),
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
fn validation_idempotent() {
    let cfg = fully_valid_config();
    let w1 = validate_config(&cfg).unwrap();
    let w2 = validate_config(&cfg).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn load_config_none_returns_default() {
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn load_config_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "default_backend = \"mock\"\nlog_level = \"warn\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn workorder_lane_patch_first_by_default() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn workorder_lane_workspace_first() {
    let wo = WorkOrderBuilder::new("test")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn workorder_workspace_mode_staged_by_default() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn workorder_vendor_dotted_key_abp_mode() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp.mode".into(), serde_json::json!("passthrough"));
    let wo = WorkOrderBuilder::new("test").config(config).build();

    assert_eq!(wo.config.vendor["abp.mode"], "passthrough");
}

#[test]
fn workorder_multiple_vendor_keys() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".into(), serde_json::json!({"mode": "mapped"}));
    config
        .vendor
        .insert("openai".into(), serde_json::json!({"temperature": 0.7}));
    let wo = WorkOrderBuilder::new("test").config(config).build();
    assert_eq!(wo.config.vendor.len(), 2);
}

#[test]
fn error_code_snake_case_serialization() {
    use abp_core::error::ErrorCode;
    let code = ErrorCode::ConfigurationError;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"configuration_error\"");
}

#[test]
fn error_code_display_uses_code_string() {
    use abp_core::error::ErrorCode;
    let code = ErrorCode::ConfigurationError;
    assert_eq!(code.to_string(), "ABP-S004");
}

#[test]
fn error_code_description_is_human_readable() {
    use abp_core::error::ErrorCode;
    let desc = ErrorCode::ConfigurationError.description();
    assert!(!desc.is_empty());
    assert!(desc.contains("configuration"));
}

#[test]
fn error_code_roundtrip_via_json() {
    use abp_core::error::ErrorCode;
    let code = ErrorCode::MissingRequiredField;
    let json = serde_json::to_string(&code).unwrap();
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(code, back);
}

#[test]
fn config_warning_equality() {
    let w1 = ConfigWarning::DeprecatedField {
        field: "x".into(),
        suggestion: Some("y".into()),
    };
    let w2 = ConfigWarning::DeprecatedField {
        field: "x".into(),
        suggestion: Some("y".into()),
    };
    assert_eq!(w1, w2);
}

#[test]
fn config_warning_inequality() {
    let w1 = ConfigWarning::LargeTimeout {
        backend: "a".into(),
        secs: 100,
    };
    let w2 = ConfigWarning::LargeTimeout {
        backend: "b".into(),
        secs: 100,
    };
    assert_ne!(w1, w2);
}
