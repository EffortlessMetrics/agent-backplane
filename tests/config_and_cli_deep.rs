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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Comprehensive tests for configuration validation and CLI command flow.
//!
//! Covers: TOML config parsing, WorkOrder construction, CLI argument parsing,
//! config validation rules, vendor config merging, and end-to-end flows.

use std::collections::BTreeMap;
use std::path::Path;

use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, apply_env_overrides, load_config,
    merge_configs, parse_toml, validate_config,
};
use abp_core::{
    ContextPacket, ExecutionLane, ExecutionMode, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
    config::{ConfigDefaults, ConfigValidator, WarningSeverity},
};
use serde_json::{Map as JsonMap, Value as JsonValue, json};

// =========================================================================
// Helper: env var guard for process-global env var tests
// =========================================================================

struct EnvGuard {
    keys: Vec<&'static str>,
}

impl EnvGuard {
    fn new(pairs: &[(&'static str, &str)]) -> Self {
        let keys: Vec<&'static str> = pairs.iter().map(|(k, _)| *k).collect();
        for (k, v) in pairs {
            unsafe { std::env::set_var(k, v) };
        }
        Self { keys }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for k in &self.keys {
            unsafe { std::env::remove_var(k) };
        }
    }
}

// =========================================================================
// Section 1: TOML config parsing (15 tests)
// =========================================================================

#[test]
fn toml_parse_valid_minimal_config() {
    let cfg = parse_toml("").unwrap();
    assert_eq!(cfg.default_backend, None);
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_parse_valid_full_config() {
    let toml = r#"
        default_backend = "mock"
        log_level = "debug"
        receipts_dir = "/tmp/receipts"
        workspace_dir = "/tmp/ws"

        [backends.mock]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["openai-sidecar.js"]
        timeout_secs = 300
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/tmp/receipts"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/ws"));
    assert_eq!(cfg.backends.len(), 2);
}

#[test]
fn toml_parse_missing_required_sidecar_command_field() {
    // command is required for sidecar; TOML parse itself should fail
    let toml = r#"
        [backends.bad]
        type = "sidecar"
        args = ["host.js"]
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_extra_fields_ignored() {
    let toml = r#"
        default_backend = "mock"
        some_unknown_field = "hello"
        another_field = 42
    "#;
    // TOML deserialization with serde's default deny_unknown_fields OFF
    // should either parse or error. Let's check the behavior.
    let result = parse_toml(toml);
    // abp-config does NOT use deny_unknown_fields, so extras are silently ignored
    assert!(result.is_ok(), "extra fields should be ignored");
    assert_eq!(result.unwrap().default_backend.as_deref(), Some("mock"));
}

#[test]
fn toml_parse_invalid_syntax_gives_parse_error() {
    let bad = "this is [not valid toml =";
    let err = parse_toml(bad).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_wrong_type_for_log_level() {
    let toml = r#"log_level = 42"#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_wrong_type_for_backends() {
    let toml = r#"backends = "not a table""#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_env_override_default_backend() {
    let _guard = EnvGuard::new(&[("ABP_DEFAULT_BACKEND", "sidecar:claude")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("sidecar:claude"));
}

#[test]
fn toml_env_override_log_level() {
    let _guard = EnvGuard::new(&[("ABP_LOG_LEVEL", "trace")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
}

#[test]
fn toml_env_override_receipts_dir() {
    let _guard = EnvGuard::new(&[("ABP_RECEIPTS_DIR", "/custom/receipts")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/custom/receipts"));
}

#[test]
fn toml_env_override_workspace_dir() {
    let _guard = EnvGuard::new(&[("ABP_WORKSPACE_DIR", "/custom/ws")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/custom/ws"));
}

#[test]
fn toml_valid_log_levels_accepted() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some(level.to_string()),
            ..Default::default()
        };
        assert!(
            validate_config(&cfg).is_ok(),
            "log level '{level}' should be valid"
        );
    }
}

#[test]
fn toml_backend_selection_mock() {
    let toml = r#"
        [backends.test_mock]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["test_mock"], BackendEntry::Mock {}));
}

#[test]
fn toml_backend_selection_sidecar() {
    let toml = r#"
        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["host.js", "--flag"]
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
            assert_eq!(args, &["host.js", "--flag"]);
            assert_eq!(*timeout_secs, Some(120));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_workspace_dir_config() {
    let toml = r#"workspace_dir = "/my/workspace""#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/my/workspace"));
}

// =========================================================================
// Section 2: WorkOrder construction (15 tests)
// =========================================================================

#[test]
fn wo_builder_minimal() {
    let wo = WorkOrderBuilder::new("do something").build();
    assert_eq!(wo.task, "do something");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert_eq!(wo.workspace.root, ".");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn wo_builder_with_model() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn wo_builder_with_lane() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn wo_builder_with_workspace_mode_passthrough() {
    let wo = WorkOrderBuilder::new("task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn wo_builder_with_budget_and_turns() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(5.0)
        .max_turns(20)
        .build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.max_turns, Some(20));
}

#[test]
fn wo_builder_vendor_config_simple() {
    let mut vendor = BTreeMap::new();
    vendor.insert("stream".to_string(), json!(true));
    vendor.insert("temperature".to_string(), json!(0.7));
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert_eq!(wo.config.vendor.get("stream"), Some(&json!(true)));
    assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.7)));
}

#[test]
fn wo_vendor_config_nested_dotted_keys() {
    // Simulate what the CLI does with dotted keys like "abp.mode"
    let mut vendor = BTreeMap::new();
    let mut abp_map = JsonMap::new();
    abp_map.insert("mode".to_string(), json!("passthrough"));
    vendor.insert("abp".to_string(), JsonValue::Object(abp_map));

    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    let abp_val = wo.config.vendor.get("abp").unwrap();
    assert_eq!(abp_val["mode"], json!("passthrough"));
}

#[test]
fn wo_vendor_config_deep_nesting() {
    let mut vendor = BTreeMap::new();
    let nested = json!({"sub": {"deep": "value"}});
    vendor.insert("top".to_string(), nested);

    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert_eq!(wo.config.vendor["top"]["sub"]["deep"], json!("value"));
}

#[test]
fn wo_mode_selection_default_is_mapped() {
    let mode = ExecutionMode::default();
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn wo_mode_passthrough_serde() {
    let json_str = r#""passthrough""#;
    let mode: ExecutionMode = serde_json::from_str(json_str).unwrap();
    assert_eq!(mode, ExecutionMode::Passthrough);
}

#[test]
fn wo_mode_mapped_serde() {
    let json_str = r#""mapped""#;
    let mode: ExecutionMode = serde_json::from_str(json_str).unwrap();
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn wo_policy_config_embedding() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        allow_network: vec![],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["Write".into()],
    };
    let wo = WorkOrderBuilder::new("task").policy(policy.clone()).build();
    assert_eq!(wo.policy.allowed_tools, vec!["Read"]);
    assert_eq!(wo.policy.disallowed_tools, vec!["Bash"]);
    assert_eq!(wo.policy.deny_read, vec!["**/.env"]);
    assert_eq!(wo.policy.deny_network, vec!["*.evil.com"]);
}

#[test]
fn wo_workspace_include_exclude() {
    let wo = WorkOrderBuilder::new("task")
        .include(vec!["src/**".into(), "Cargo.toml".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["src/**", "Cargo.toml"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn wo_workspace_root_custom() {
    let wo = WorkOrderBuilder::new("task").root("/my/project").build();
    assert_eq!(wo.workspace.root, "/my/project");
}

#[test]
fn wo_apply_defaults_fills_missing() {
    let mut wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.config.model.is_none());

    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.max_turns, Some(25));
    assert_eq!(wo.config.max_budget_usd, Some(1.0));
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

// =========================================================================
// Section 3: CLI argument parsing (10 tests)
// =========================================================================
//
// Since the CLI binary uses clap::Parser in main.rs (not a library-exported
// struct), we test CLI behavior via assert_cmd where possible and verify
// the library-level command functions directly.

#[test]
fn cli_schema_work_order_produces_valid_json() {
    use abp_cli::commands::{SchemaKind, schema_json};
    let json = schema_json(SchemaKind::WorkOrder).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.is_object());
}

#[test]
fn cli_schema_receipt_produces_valid_json() {
    use abp_cli::commands::{SchemaKind, schema_json};
    let json = schema_json(SchemaKind::Receipt).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.is_object());
}

#[test]
fn cli_schema_config_produces_valid_json() {
    use abp_cli::commands::{SchemaKind, schema_json};
    let json = schema_json(SchemaKind::Config).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.is_object());
}

#[test]
fn cli_validate_file_detects_work_order() {
    use abp_cli::commands::{ValidatedType, validate_file};
    let wo = WorkOrderBuilder::new("test").build();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wo.json");
    std::fs::write(&path, serde_json::to_string_pretty(&wo).unwrap()).unwrap();
    assert_eq!(validate_file(&path).unwrap(), ValidatedType::WorkOrder);
}

#[test]
fn cli_validate_file_detects_receipt() {
    use abp_cli::commands::{ValidatedType, validate_file};
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    assert_eq!(validate_file(&path).unwrap(), ValidatedType::Receipt);
}

#[test]
fn cli_validate_file_rejects_arbitrary_json() {
    use abp_cli::commands::validate_file;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unknown.json");
    std::fs::write(&path, r#"{"random": true}"#).unwrap();
    assert!(validate_file(&path).is_err());
}

#[test]
fn cli_validate_file_rejects_invalid_json() {
    use abp_cli::commands::validate_file;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "not json at all").unwrap();
    assert!(validate_file(&path).is_err());
}

#[test]
fn cli_config_check_defaults_ok() {
    use abp_cli::commands::config_check;
    let diags = config_check(None).unwrap();
    assert!(diags.iter().any(|d| d.contains("ok")));
}

#[test]
fn cli_config_check_bad_toml_reports_error() {
    use abp_cli::commands::config_check;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "[[[[broken").unwrap();
    let diags = config_check(Some(&path)).unwrap();
    assert!(diags.iter().any(|d| d.starts_with("error:")));
}

#[test]
fn cli_receipt_diff_identical_receipts() {
    use abp_cli::commands::receipt_diff;
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("r1.json");
    let p2 = dir.path().join("r2.json");
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    std::fs::write(&p1, &json).unwrap();
    std::fs::write(&p2, &json).unwrap();
    let diff = receipt_diff(&p1, &p2).unwrap();
    assert_eq!(diff, "no differences");
}

// =========================================================================
// Section 4: Config validation rules (10 tests)
// =========================================================================

#[test]
fn config_validation_empty_task_is_error() {
    let wo = WorkOrderBuilder::new("   ").build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "task" && matches!(w.severity, WarningSeverity::Error)),
        "empty task should produce an Error warning"
    );
}

#[test]
fn config_validation_zero_max_turns_is_error() {
    let validator = ConfigValidator::new();
    let mut wo2 = WorkOrderBuilder::new("task").build();
    wo2.config.max_turns = Some(0);
    let warnings2 = validator.validate_work_order(&wo2);
    assert!(
        warnings2
            .iter()
            .any(|w| w.field == "config.max_turns" && matches!(w.severity, WarningSeverity::Error))
    );
}

#[test]
fn config_validation_negative_budget_is_error() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.config.max_budget_usd = Some(-1.0);
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(warnings.iter().any(
        |w| w.field == "config.max_budget_usd" && matches!(w.severity, WarningSeverity::Error)
    ));
}

#[test]
fn config_validation_zero_budget_is_error() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.config.max_budget_usd = Some(0.0);
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(warnings.iter().any(
        |w| w.field == "config.max_budget_usd" && matches!(w.severity, WarningSeverity::Error)
    ));
}

#[test]
fn config_validation_empty_model_name_is_error() {
    let wo = WorkOrderBuilder::new("task").model("   ").build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "config.model" && matches!(w.severity, WarningSeverity::Error))
    );
}

#[test]
fn config_validation_duplicate_tools_warning() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Read".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(warnings.iter().any(
        |w| w.field == "policy.allowed_tools" && matches!(w.severity, WarningSeverity::Warning)
    ));
}

#[test]
fn config_validation_empty_glob_in_deny_read() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "  ".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "policy.deny_read" && matches!(w.severity, WarningSeverity::Error))
    );
}

#[test]
fn config_validation_valid_minimal_passes() {
    let wo = WorkOrderBuilder::new("Fix the bug").build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    let errors: Vec<_> = warnings
        .iter()
        .filter(|w| matches!(w.severity, WarningSeverity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "minimal valid config should have no errors"
    );
}

#[test]
fn config_validation_valid_full_passes() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec!["Bash".into()],
    };
    let wo = WorkOrderBuilder::new("Implement auth")
        .model("claude-3.5-sonnet")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/project")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .max_budget_usd(10.0)
        .max_turns(50)
        .policy(policy)
        .build();
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    let errors: Vec<_> = warnings
        .iter()
        .filter(|w| matches!(w.severity, WarningSeverity::Error))
        .collect();
    assert!(errors.is_empty(), "full valid config should have no errors");
}

#[test]
fn config_validation_empty_vendor_key_is_error() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.config.vendor.insert("  ".to_string(), json!("value"));
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "config.vendor" && matches!(w.severity, WarningSeverity::Error))
    );
}

// =========================================================================
// Section 5: Additional config + CLI cross-cutting tests (11+ tests)
// =========================================================================

#[test]
fn config_merge_overlay_overrides_default_backend() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("sidecar:claude".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("sidecar:claude"));
}

#[test]
fn config_merge_combines_backends_from_both() {
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
fn config_merge_overlay_backend_wins_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python3".into(),
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
fn config_validation_invalid_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn config_validation_empty_sidecar_command() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "bad".into(),
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
fn config_validation_timeout_zero_rejected() {
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
fn config_validation_timeout_exceeds_max() {
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
fn config_validation_large_timeout_warning() {
    let cfg = BackplaneConfig {
        default_backend: Some("sc".into()),
        receipts_dir: Some("/tmp".into()),
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(7200),
            },
        )]),
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
    );
}

#[test]
fn config_load_none_returns_default() {
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
    assert!(cfg.backends.is_empty());
}

#[test]
fn config_load_missing_file_error() {
    let err = load_config(Some(Path::new("/does/not/exist.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn config_load_from_tempfile() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "sidecar:node"
log_level = "warn"

[backends.mock]
type = "mock"
"#,
    )
    .unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("sidecar:node"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    assert_eq!(cfg.backends.len(), 1);
}

#[test]
fn config_roundtrip_toml_serialize_deserialize() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn config_error_display_messages() {
    let e = ConfigError::FileNotFound {
        path: "/foo.toml".into(),
    };
    assert!(e.to_string().contains("/foo.toml"));

    let e = ConfigError::ParseError {
        reason: "unexpected token".into(),
    };
    assert!(e.to_string().contains("unexpected token"));

    let e = ConfigError::ValidationError {
        reasons: vec!["bad thing".into()],
    };
    assert!(e.to_string().contains("bad thing"));

    let e = ConfigError::MergeConflict {
        reason: "conflicting backends".into(),
    };
    assert!(e.to_string().contains("conflicting backends"));
}

#[test]
fn config_warning_display_messages() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: Some("new_field".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old_field"));
    assert!(s.contains("new_field"));

    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "will not persist".into(),
    };
    assert!(w.to_string().contains("receipts_dir"));

    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 7200,
    };
    assert!(w.to_string().contains("7200"));
}

#[test]
fn config_validation_missing_default_backend_advisory() {
    let cfg = BackplaneConfig {
        default_backend: None,
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend")));
}

#[test]
fn config_validation_missing_receipts_dir_advisory() {
    let cfg = BackplaneConfig {
        receipts_dir: None,
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir")));
}

#[test]
fn config_validation_valid_with_all_fields_set() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/receipts".into()),
        backends: BTreeMap::from([
            ("mock".into(), BackendEntry::Mock {}),
            (
                "node".into(),
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(300),
                },
            ),
        ]),
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    // Should have no missing-field advisories
    let missing: Vec<_> = warnings
        .iter()
        .filter(|w| matches!(w, ConfigWarning::MissingOptionalField { .. }))
        .collect();
    assert!(missing.is_empty());
}

#[test]
fn wo_serde_roundtrip_json() {
    let wo = WorkOrderBuilder::new("test task")
        .model("gpt-4")
        .lane(ExecutionLane::WorkspaceFirst)
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let deserialized: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.task, "test task");
    assert_eq!(deserialized.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(deserialized.config.max_turns, Some(10));
}

#[test]
fn wo_config_defaults_values() {
    assert_eq!(ConfigDefaults::default_max_turns(), 25);
    assert_eq!(ConfigDefaults::default_max_budget(), 1.0);
    assert_eq!(ConfigDefaults::default_model(), "gpt-4");
}

#[test]
fn wo_apply_defaults_does_not_override_existing() {
    let mut wo = WorkOrderBuilder::new("task")
        .model("claude-3.5-sonnet")
        .max_turns(50)
        .max_budget_usd(10.0)
        .build();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.model.as_deref(), Some("claude-3.5-sonnet"));
    assert_eq!(wo.config.max_turns, Some(50));
    assert_eq!(wo.config.max_budget_usd, Some(10.0));
}

#[test]
fn cli_inspect_receipt_valid_hash() {
    use abp_cli::commands::inspect_receipt_file;
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    let (r, valid) = inspect_receipt_file(&path).unwrap();
    assert!(valid);
    assert_eq!(r.receipt_sha256, receipt.receipt_sha256);
}

#[test]
fn cli_inspect_receipt_tampered_hash() {
    use abp_cli::commands::inspect_receipt_file;
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("0".repeat(64));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    let (_r, valid) = inspect_receipt_file(&path).unwrap();
    assert!(!valid);
}

#[test]
fn cli_receipt_diff_different_backends() {
    use abp_cli::commands::receipt_diff;
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = abp_core::ReceiptBuilder::new("sidecar:node")
        .outcome(abp_core::Outcome::Failed)
        .with_hash()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("r1.json");
    let p2 = dir.path().join("r2.json");
    std::fs::write(&p1, serde_json::to_string_pretty(&r1).unwrap()).unwrap();
    std::fs::write(&p2, serde_json::to_string_pretty(&r2).unwrap()).unwrap();
    let diff = receipt_diff(&p1, &p2).unwrap();
    assert!(diff.contains("outcome"));
    assert!(diff.contains("backend"));
}

#[test]
fn config_empty_backend_name_rejected() {
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
fn config_example_toml_parses() {
    let content = include_str!("../backplane.example.toml");
    let config: abp_cli::config::BackplaneConfig =
        toml::from_str(content).expect("example config should parse");
    assert!(!config.backends.is_empty());
}

#[test]
fn wo_context_packet_default_empty() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
}

#[test]
fn wo_context_packet_with_files() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![abp_core::ContextSnippet {
            name: "hint".into(),
            content: "Look at the auth module".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    assert_eq!(wo.context.files.len(), 2);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "hint");
}
