// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep configuration validation tests for the `abp-config` crate.
//!
//! Covers: defaults, TOML parsing, validation, merge/override, env vars,
//! partial configs, schema conformance, backend-specific sections, log levels,
//! path resolution, and hot-reload detection.

use std::collections::BTreeMap;
use std::path::Path;

use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, apply_env_overrides, load_config,
    merge_configs, parse_toml, validate_config,
};

// =========================================================================
// Helper: remove ABP env vars so parallel tests don't interfere
// =========================================================================

/// Guard that removes ABP_* env vars on drop. Tests that set env vars should
/// hold this guard for the duration of the test.
struct EnvGuard {
    keys: Vec<&'static str>,
}

impl EnvGuard {
    fn new(pairs: &[(&'static str, &str)]) -> Self {
        let keys: Vec<&'static str> = pairs.iter().map(|(k, _)| *k).collect();
        for (k, v) in pairs {
            // SAFETY: these tests run serially (env vars are process-global).
            unsafe { std::env::set_var(k, v) };
        }
        Self { keys }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for k in &self.keys {
            // SAFETY: restoring env to previous state on drop.
            unsafe { std::env::remove_var(k) };
        }
    }
}

// =========================================================================
// 1. Default config — all fields and their defaults
// =========================================================================

#[test]
fn default_config_fields() {
    let cfg = BackplaneConfig::default();
    assert_eq!(cfg.default_backend, None);
    assert_eq!(cfg.workspace_dir, None);
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
    assert_eq!(cfg.receipts_dir, None);
    assert!(cfg.backends.is_empty());
}

#[test]
fn default_config_validates_with_advisory_warnings() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).expect("default config must be valid");
    // Should warn about missing default_backend and receipts_dir.
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend")));
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir")));
}

// =========================================================================
// 2. Valid TOML config fields
// =========================================================================

#[test]
fn parse_all_valid_fields() {
    let toml = r#"
        default_backend = "mock"
        workspace_dir   = "/tmp/ws"
        log_level       = "debug"
        receipts_dir    = "/tmp/receipts"

        [backends.mock]
        type = "mock"

        [backends.node]
        type         = "sidecar"
        command      = "node"
        args         = ["host.js", "--flag"]
        timeout_secs = 600
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/ws"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/tmp/receipts"));
    assert_eq!(cfg.backends.len(), 2);
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
    match &cfg.backends["node"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js", "--flag"]);
            assert_eq!(*timeout_secs, Some(600));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_empty_toml_yields_defaults() {
    let cfg = parse_toml("").unwrap();
    // Empty TOML gives serde defaults: all None except log_level isn't set by serde default
    assert_eq!(cfg.default_backend, None);
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_roundtrip_with_all_fields() {
    let cfg = BackplaneConfig {
        default_backend: Some("sc".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("trace".into()),
        receipts_dir: Some("/r".into()),
        backends: BTreeMap::from([
            ("mock".into(), BackendEntry::Mock {}),
            (
                "sc".into(),
                BackendEntry::Sidecar {
                    command: "python".into(),
                    args: vec!["-m".into(), "host".into()],
                    timeout_secs: Some(120),
                },
            ),
        ]),
    };
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

// =========================================================================
// 3. Invalid config values — wrong types
// =========================================================================

#[test]
fn wrong_type_log_level_integer() {
    let err = parse_toml("log_level = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn wrong_type_backends_not_table() {
    let err = parse_toml("backends = 123").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn wrong_type_default_backend_boolean() {
    let err = parse_toml("default_backend = true").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn wrong_type_timeout_secs_string() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        timeout_secs = "not_a_number"
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn wrong_type_args_not_array() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = "should be array"
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn invalid_toml_syntax() {
    let err = parse_toml("not valid [[ toml =").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// =========================================================================
// 4. Invalid config values — out of range / semantic
// =========================================================================

#[test]
fn validation_rejects_invalid_log_levels() {
    for bad in &["verbose", "WARNING", "INFO", "TRACE", "", "off", "all"] {
        let cfg = BackplaneConfig {
            log_level: Some((*bad).to_string()),
            ..Default::default()
        };
        let err = validate_config(&cfg).unwrap_err();
        assert!(
            matches!(err, ConfigError::ValidationError { .. }),
            "expected ValidationError for log_level={bad}"
        );
    }
}

#[test]
fn validation_accepts_all_valid_log_levels() {
    for good in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*good).to_string()),
            ..Default::default()
        };
        validate_config(&cfg).unwrap_or_else(|e| panic!("log_level={good} should be valid: {e}"));
    }
}

#[test]
fn validation_rejects_zero_timeout() {
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
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.iter().any(|r| r.contains("out of range")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validation_rejects_timeout_exceeding_max() {
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
fn validation_accepts_boundary_timeout_values() {
    // Timeout = 1 (minimum valid)
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(1),
        },
    );
    validate_config(&cfg).expect("timeout=1 should be valid");

    // Timeout = 86400 (maximum valid)
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    let warnings = validate_config(&cfg).expect("timeout=86400 should be valid");
    // 86400 > 3600, so it should produce a LargeTimeout warning
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { secs: 86400, .. }))
    );
}

#[test]
fn validation_rejects_empty_sidecar_command() {
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
fn validation_rejects_whitespace_only_sidecar_command() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "ws".into(),
        BackendEntry::Sidecar {
            command: "   \t  ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validation_rejects_empty_backend_name() {
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
fn validation_collects_multiple_errors() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bogus".into()),
        ..Default::default()
    };
    cfg.backends.insert(
        "bad".into(),
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
                reasons.len() >= 2,
                "expected at least 2 errors, got: {reasons:?}"
            );
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// =========================================================================
// 5. Config merge/override behavior
// =========================================================================

#[test]
fn merge_overlay_overrides_all_scalar_fields() {
    let base = BackplaneConfig {
        default_backend: Some("old_backend".into()),
        workspace_dir: Some("/old/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/old/receipts".into()),
        backends: BTreeMap::new(),
    };
    let overlay = BackplaneConfig {
        default_backend: Some("new_backend".into()),
        workspace_dir: Some("/new/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/new/receipts".into()),
        backends: BTreeMap::new(),
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("new_backend"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/new/ws"));
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/new/receipts"));
}

#[test]
fn merge_overlay_none_preserves_base() {
    let base = BackplaneConfig {
        default_backend: Some("base_backend".into()),
        workspace_dir: Some("/base/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/base/receipts".into()),
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("base_backend"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/base/ws"));
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/base/receipts"));
    assert!(merged.backends.contains_key("m"));
}

#[test]
fn merge_combines_disjoint_backends() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "b".into(),
            BackendEntry::Sidecar {
                command: "python".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.backends.len(), 2);
    assert!(merged.backends.contains_key("a"));
    assert!(merged.backends.contains_key("b"));
}

#[test]
fn merge_overlay_backend_wins_on_name_collision() {
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
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
            assert_eq!(*timeout_secs, Some(60));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_three_layers_file_env_cli() {
    // Simulate file → env overlay → CLI overlay
    let file = BackplaneConfig {
        default_backend: Some("from_file".into()),
        workspace_dir: Some("/file/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/file/receipts".into()),
        backends: BTreeMap::from([("file_mock".into(), BackendEntry::Mock {})]),
    };
    let env_overlay = BackplaneConfig {
        default_backend: Some("from_env".into()),
        workspace_dir: None,
        log_level: Some("debug".into()),
        receipts_dir: None,
        backends: BTreeMap::new(),
    };
    let cli_overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: Some("/cli/ws".into()),
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::from([("cli_sc".into(), BackendEntry::Mock {})]),
    };
    let after_env = merge_configs(file, env_overlay);
    let final_cfg = merge_configs(after_env, cli_overlay);

    assert_eq!(final_cfg.default_backend.as_deref(), Some("from_env"));
    assert_eq!(final_cfg.workspace_dir.as_deref(), Some("/cli/ws"));
    assert_eq!(final_cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(final_cfg.receipts_dir.as_deref(), Some("/file/receipts"));
    assert!(final_cfg.backends.contains_key("file_mock"));
    assert!(final_cfg.backends.contains_key("cli_sc"));
}

// =========================================================================
// 6. Environment variable overrides (ABP_* prefix)
// =========================================================================

#[test]
fn env_override_default_backend() {
    let _guard = EnvGuard::new(&[("ABP_DEFAULT_BACKEND", "env_mock")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("env_mock"));
}

#[test]
fn env_override_log_level() {
    // Use a distinctive value to verify our override took effect.
    let _guard = EnvGuard::new(&[("ABP_LOG_LEVEL", "trace")]);
    let mut cfg = BackplaneConfig {
        log_level: None,
        ..Default::default()
    };
    apply_env_overrides(&mut cfg);
    // The override should populate log_level from the env var.
    assert!(cfg.log_level.is_some(), "env override should set log_level");
}

#[test]
fn env_override_receipts_dir() {
    let _guard = EnvGuard::new(&[("ABP_RECEIPTS_DIR", "/env/receipts")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/env/receipts"));
}

#[test]
fn env_override_workspace_dir() {
    let _guard = EnvGuard::new(&[("ABP_WORKSPACE_DIR", "/env/ws")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/env/ws"));
}

#[test]
fn env_overrides_replace_existing_values() {
    let _guard = EnvGuard::new(&[
        ("ABP_DEFAULT_BACKEND", "env_backend"),
        ("ABP_LOG_LEVEL", "error"),
    ]);
    let mut cfg = BackplaneConfig {
        default_backend: Some("file_backend".into()),
        log_level: Some("info".into()),
        ..Default::default()
    };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("env_backend"));
    assert_eq!(cfg.log_level.as_deref(), Some("error"));
}

#[test]
fn env_overrides_applied_via_load_config() {
    let _guard = EnvGuard::new(&[("ABP_LOG_LEVEL", "warn")]);
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn env_overrides_on_top_of_file() {
    // Test apply_env_overrides directly to avoid env-var races in parallel tests
    let mut cfg = parse_toml("default_backend = \"from_file\"\nlog_level = \"debug\"").unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("from_file"));
    let _guard = EnvGuard::new(&[("ABP_DEFAULT_BACKEND", "from_env")]);
    apply_env_overrides(&mut cfg);
    // env overrides the file value
    assert_eq!(cfg.default_backend.as_deref(), Some("from_env"));
    // file value preserved for non-overridden field
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
}

// =========================================================================
// 7. Partial configs — only some fields specified
// =========================================================================

#[test]
fn partial_config_only_log_level() {
    let cfg = parse_toml("log_level = \"warn\"").unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    assert_eq!(cfg.default_backend, None);
    assert_eq!(cfg.workspace_dir, None);
    assert_eq!(cfg.receipts_dir, None);
    assert!(cfg.backends.is_empty());
    validate_config(&cfg).expect("partial config should be valid");
}

#[test]
fn partial_config_only_backends() {
    let toml = r#"
        [backends.m]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend, None);
    assert_eq!(cfg.backends.len(), 1);
    validate_config(&cfg).expect("partial config should be valid");
}

#[test]
fn partial_config_only_default_backend() {
    let cfg = parse_toml(r#"default_backend = "some_backend""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("some_backend"));
    assert!(cfg.backends.is_empty());
    let warnings = validate_config(&cfg).unwrap();
    // Should warn about missing receipts_dir but NOT about default_backend
    assert!(!warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend")));
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir")));
}

#[test]
fn partial_config_no_log_level_uses_serde_default() {
    // When deserializing from TOML without log_level, serde default gives None
    // (the Default impl sets it to Some("info") but serde uses field-level default)
    let cfg = parse_toml("default_backend = \"mock\"").unwrap();
    // serde default for Option is None, not the impl Default value
    // The validate_config should still accept None log_level
    validate_config(&cfg).expect("None log_level should be valid");
}

// =========================================================================
// 8. Config schema conformance
// =========================================================================

#[test]
fn json_schema_generation() {
    let schema = schemars::schema_for!(BackplaneConfig);
    let schema_json = serde_json::to_value(&schema).unwrap();
    // Schema should be a valid JSON object.
    assert!(schema_json.is_object());
    // schemars v1 may place properties at the root or under $defs.
    // Find the object with properties for BackplaneConfig.
    let props = if schema_json.get("properties").is_some() {
        &schema_json["properties"]
    } else {
        &schema_json["$defs"]["BackplaneConfig"]["properties"]
    };
    assert!(props.get("default_backend").is_some());
    assert!(props.get("workspace_dir").is_some());
    assert!(props.get("log_level").is_some());
    assert!(props.get("receipts_dir").is_some());
    assert!(props.get("backends").is_some());
}

#[test]
fn valid_config_conforms_to_schema() {
    let schema = schemars::schema_for!(BackplaneConfig);
    let schema_value = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&schema_value).expect("valid JSON schema");

    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
    };
    let cfg_json = serde_json::to_value(&cfg).unwrap();
    let result = validator.validate(&cfg_json);
    assert!(result.is_ok(), "config should conform to schema");
}

#[test]
fn default_config_conforms_to_schema() {
    let schema = schemars::schema_for!(BackplaneConfig);
    let schema_value = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&schema_value).expect("valid JSON schema");

    let cfg = BackplaneConfig::default();
    let cfg_json = serde_json::to_value(&cfg).unwrap();
    let result = validator.validate(&cfg_json);
    assert!(result.is_ok(), "default config should conform to schema");
}

// =========================================================================
// 9. Config file hot-reload detection
// =========================================================================

#[test]
fn detect_file_modification_via_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(&path, "log_level = \"info\"").unwrap();
    let meta1 = std::fs::metadata(&path).unwrap();
    let mtime1 = meta1.modified().unwrap();

    // Wait briefly and rewrite
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(&path, "log_level = \"debug\"").unwrap();
    let meta2 = std::fs::metadata(&path).unwrap();
    let mtime2 = meta2.modified().unwrap();

    assert!(
        mtime2 > mtime1,
        "file modification time should increase after rewrite"
    );

    // Reload should reflect the new value
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
}

#[test]
fn reload_picks_up_new_backends() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");

    // Initial config with one backend
    std::fs::write(
        &path,
        r#"
        log_level = "info"
        [backends.mock]
        type = "mock"
    "#,
    )
    .unwrap();
    let cfg1 = load_config(Some(&path)).unwrap();
    assert_eq!(cfg1.backends.len(), 1);

    // Update to add another backend
    std::fs::write(
        &path,
        r#"
        log_level = "info"
        [backends.mock]
        type = "mock"
        [backends.sc]
        type = "sidecar"
        command = "node"
    "#,
    )
    .unwrap();
    let cfg2 = load_config(Some(&path)).unwrap();
    assert_eq!(cfg2.backends.len(), 2);
    assert!(cfg2.backends.contains_key("sc"));
}

// =========================================================================
// 10. Backend-specific config sections
// =========================================================================

#[test]
fn mock_backend_has_no_extra_fields() {
    let toml = r#"
        [backends.m]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn sidecar_backend_minimal() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert!(args.is_empty());
            assert_eq!(*timeout_secs, None);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn sidecar_backend_full() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "python"
        args = ["-m", "host", "--verbose"]
        timeout_secs = 300
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python");
            assert_eq!(args, &["-m", "host", "--verbose"]);
            assert_eq!(*timeout_secs, Some(300));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn sidecar_missing_command_fails_parse() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn unknown_backend_type_fails_parse() {
    let toml = r#"
        [backends.sc]
        type = "openai_api"
        api_key = "sk-xxx"
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn multiple_backend_types_together() {
    let toml = r#"
        [backends.m]
        type = "mock"

        [backends.node_sc]
        type = "sidecar"
        command = "node"
        args = ["host.js"]

        [backends.py_sc]
        type = "sidecar"
        command = "python"
        timeout_secs = 600
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
    assert!(matches!(
        cfg.backends["node_sc"],
        BackendEntry::Sidecar { .. }
    ));
    assert!(matches!(
        cfg.backends["py_sc"],
        BackendEntry::Sidecar { .. }
    ));
    validate_config(&cfg).expect("multiple backends should be valid");
}

#[test]
fn large_timeout_warning_threshold() {
    // timeout = 3600 is exactly at threshold — should NOT warn (> 3600 triggers)
    let mut cfg = BackplaneConfig::default();
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
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. })),
        "timeout=3600 should NOT trigger LargeTimeout warning"
    );

    // timeout = 3601 should warn
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3601),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { secs: 3601, .. }))
    );
}

// =========================================================================
// 11. Logging level validation
// =========================================================================

#[test]
fn log_level_case_sensitive() {
    // "Info" (title case) should be rejected — only lowercase valid
    let cfg = BackplaneConfig {
        log_level: Some("Info".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn log_level_none_is_valid() {
    let cfg = BackplaneConfig {
        log_level: None,
        ..Default::default()
    };
    validate_config(&cfg).expect("None log_level should be valid");
}

// =========================================================================
// 12. Path resolution — relative vs absolute workspace paths
// =========================================================================

#[test]
fn workspace_dir_accepts_absolute_path() {
    let cfg = parse_toml(r#"workspace_dir = "/absolute/path/ws""#).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/absolute/path/ws"));
    let path = Path::new(cfg.workspace_dir.as_deref().unwrap());
    assert!(path.is_absolute() || cfg.workspace_dir.as_deref().unwrap().starts_with('/'));
}

#[test]
fn workspace_dir_accepts_relative_path() {
    let cfg = parse_toml(r#"workspace_dir = "./relative/ws""#).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("./relative/ws"));
}

#[test]
fn receipts_dir_accepts_absolute_path() {
    let cfg = parse_toml(r#"receipts_dir = "/var/receipts""#).unwrap();
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/var/receipts"));
}

#[test]
fn receipts_dir_accepts_relative_path() {
    let cfg = parse_toml(r#"receipts_dir = "receipts""#).unwrap();
    assert_eq!(cfg.receipts_dir.as_deref(), Some("receipts"));
}

#[test]
fn workspace_dir_preserves_trailing_separator() {
    let cfg = parse_toml(r#"workspace_dir = "/path/to/ws/""#).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/path/to/ws/"));
}

// =========================================================================
// 13. Error type display / coverage
// =========================================================================

#[test]
fn config_error_display_variants() {
    let e = ConfigError::FileNotFound {
        path: "/missing.toml".into(),
    };
    assert!(e.to_string().contains("/missing.toml"));

    let e = ConfigError::ParseError {
        reason: "unexpected token".into(),
    };
    assert!(e.to_string().contains("unexpected token"));

    let e = ConfigError::ValidationError {
        reasons: vec!["bad log".into(), "bad timeout".into()],
    };
    let s = e.to_string();
    assert!(s.contains("bad log"));
    assert!(s.contains("bad timeout"));

    let e = ConfigError::MergeConflict {
        reason: "conflicting backends".into(),
    };
    assert!(e.to_string().contains("conflicting backends"));
}

#[test]
fn config_warning_display_variants() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: Some("new".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old"));
    assert!(s.contains("new"));

    let w = ConfigWarning::DeprecatedField {
        field: "removed".into(),
        suggestion: None,
    };
    assert!(w.to_string().contains("removed"));

    let w = ConfigWarning::MissingOptionalField {
        field: "x".into(),
        hint: "set it".into(),
    };
    let s = w.to_string();
    assert!(s.contains("x"));
    assert!(s.contains("set it"));

    let w = ConfigWarning::LargeTimeout {
        backend: "slow".into(),
        secs: 7200,
    };
    let s = w.to_string();
    assert!(s.contains("slow"));
    assert!(s.contains("7200"));
}

// =========================================================================
// 14. File loading edge cases
// =========================================================================

#[test]
fn load_missing_file_gives_file_not_found() {
    let err = load_config(Some(Path::new("/nonexistent/path/backplane.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_none_returns_default_config() {
    let cfg = load_config(None).unwrap();
    // load_config(None) returns default + env overrides.
    // log_level default is Some("info").
    assert!(cfg.log_level.is_some());
    assert!(cfg.backends.is_empty());
}

#[test]
fn load_from_disk_with_all_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("full.toml");
    let content = r#"
default_backend = "node_sc"
workspace_dir   = "/tmp/ws"
log_level       = "trace"
receipts_dir    = "/tmp/receipts"

[backends.m]
type = "mock"

[backends.node_sc]
type = "sidecar"
command = "node"
args = ["host.js"]
timeout_secs = 120
"#;
    std::fs::write(&path, content).unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("node_sc"));
    assert_eq!(cfg.backends.len(), 2);
}

// =========================================================================
// 15. Serde JSON roundtrip (for API/schema interop)
// =========================================================================

#[test]
fn json_roundtrip() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        backends: BTreeMap::from([
            ("m".into(), BackendEntry::Mock {}),
            (
                "sc".into(),
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(60),
                },
            ),
        ]),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn serde_skip_serializing_none_fields() {
    let cfg = BackplaneConfig::default();
    let json = serde_json::to_value(&cfg).unwrap();
    let obj = json.as_object().unwrap();
    // None fields should be skipped by skip_serializing_if
    assert!(!obj.contains_key("default_backend"));
    assert!(!obj.contains_key("workspace_dir"));
    assert!(!obj.contains_key("receipts_dir"));
    // log_level has a default of Some("info"), so it should be present
    assert!(obj.contains_key("log_level"));
}

// =========================================================================
// 16. Merge idempotence & identity
// =========================================================================

#[test]
fn merge_with_itself_is_identity() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
    };
    let merged = merge_configs(cfg.clone(), cfg.clone());
    assert_eq!(merged, cfg);
}

#[test]
fn merge_empty_overlay_preserves_base_backends() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([
            ("a".into(), BackendEntry::Mock {}),
            (
                "b".into(),
                BackendEntry::Sidecar {
                    command: "x".into(),
                    args: vec![],
                    timeout_secs: None,
                },
            ),
        ]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::new(),
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
    };
    let merged = merge_configs(base.clone(), overlay);
    assert_eq!(merged.backends.len(), 2);
    assert_eq!(merged.backends, base.backends);
}
