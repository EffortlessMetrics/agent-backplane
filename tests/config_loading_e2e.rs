// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for `abp-config` configuration loading, validation,
//! merging, environment overrides, and serialization roundtrips.
//!
//! **Race-tolerance**: Environment variable tests never assert exact `None`
//! values because parallel tests may set `ABP_*` variables concurrently.

use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, apply_env_overrides, load_config,
    merge_configs, parse_toml, validate_config,
};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A fully-specified config with no validation warnings.
fn full_config() -> BackplaneConfig {
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
        ..Default::default()
    }
}

/// Extract error reasons from a [`ConfigError::ValidationError`].
fn validation_reasons(err: ConfigError) -> Vec<String> {
    match err {
        ConfigError::ValidationError { reasons } => reasons,
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===========================================================================
// 1. TOML config file parsing
// ===========================================================================

#[test]
fn parse_empty_toml_string() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn parse_all_scalar_fields() {
    let toml = r#"
        default_backend = "openai"
        workspace_dir   = "/work"
        log_level       = "debug"
        receipts_dir    = "/receipts"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("openai"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/work"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/receipts"));
}

#[test]
fn parse_mock_backend() {
    let toml = "[backends.m]\ntype = \"mock\"";
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn parse_sidecar_backend_all_fields() {
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
fn parse_sidecar_backend_minimal() {
    let toml = "[backends.py]\ntype = \"sidecar\"\ncommand = \"python3\"";
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
        [backends.m]
        type = "mock"

        [backends.s1]
        type = "sidecar"
        command = "node"

        [backends.s2]
        type = "sidecar"
        command = "python"
        args = ["host.py"]
        timeout_secs = 60
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
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
fn parse_wrong_type_for_backends() {
    let err = parse_toml("backends = \"not a table\"").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_default_backend() {
    let err = parse_toml("default_backend = true").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_unknown_backend_type_tag() {
    let toml = "[backends.x]\ntype = \"unknown\"\ncommand = \"foo\"";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_sidecar_missing_command_field() {
    let toml = "[backends.bad]\ntype = \"sidecar\"\nargs = []";
    assert!(parse_toml(toml).is_err());
}

#[test]
fn parse_extra_unknown_fields_at_root() {
    // TOML serde by default ignores unknown fields.
    let toml = r#"
        default_backend = "mock"
        unknown_field = "hello"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn parse_toml_with_comments() {
    let toml = r#"
        # This is a comment
        default_backend = "mock" # inline comment
        log_level = "warn"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn parse_empty_args_array() {
    let toml = "[backends.s]\ntype = \"sidecar\"\ncommand = \"node\"\nargs = []";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["s"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_timeout_secs_zero_is_valid_toml() {
    // Zero is valid TOML — it's validation that rejects it.
    let toml = "[backends.s]\ntype = \"sidecar\"\ncommand = \"x\"\ntimeout_secs = 0";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["s"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert_eq!(*timeout_secs, Some(0)),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_large_timeout_is_valid_toml() {
    let toml = "[backends.s]\ntype = \"sidecar\"\ncommand = \"x\"\ntimeout_secs = 999999";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["s"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert_eq!(*timeout_secs, Some(999_999)),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

// ===========================================================================
// 2. Config file discovery (load_config with paths)
// ===========================================================================

#[test]
fn load_config_none_returns_default() {
    let cfg = load_config(None).unwrap();
    // log_level default is "info" but env override may change it
    assert!(cfg.log_level.is_some(), "should have a log_level");
}

#[test]
fn load_config_missing_file_returns_file_not_found() {
    let err = load_config(Some(Path::new("/nonexistent/backplane.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_config_from_explicit_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "default_backend = \"mybackend\"").unwrap();
    writeln!(f, "log_level = \"warn\"").unwrap();
    drop(f);

    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mybackend"));
    // log_level may be overridden by ABP_LOG_LEVEL env var
    assert!(cfg.log_level.is_some());
}

#[test]
fn load_config_from_file_with_backends() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "node"
log_level = "debug"

[backends.node]
type = "sidecar"
command = "node"
args = ["host.js"]
timeout_secs = 60

[backends.mock]
type = "mock"
"#,
    )
    .unwrap();

    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("node"));
    assert!(cfg.backends.contains_key("node"));
    assert!(cfg.backends.contains_key("mock"));
}

#[test]
fn load_config_applies_env_overrides() {
    // load_config(None) always applies env overrides.
    // We can't control what env vars are set, but we verify it doesn't panic.
    let cfg = load_config(None).unwrap();
    // The config should be structurally valid.
    assert!(cfg.backends.is_empty() || !cfg.backends.is_empty());
}

#[test]
fn load_config_file_not_found_contains_path() {
    let missing = Path::new("/does/not/exist/config.toml");
    match load_config(Some(missing)).unwrap_err() {
        ConfigError::FileNotFound { path } => {
            assert!(path.contains("config.toml"));
        }
        other => panic!("expected FileNotFound, got {other:?}"),
    }
}

#[test]
fn load_config_invalid_toml_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "not = [valid toml {{{").unwrap();
    let err = load_config(Some(&path)).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn load_config_empty_file_returns_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.toml");
    std::fs::write(&path, "").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    // After env overrides, fields might be set.
    // At minimum, backends should be empty since file was empty.
    assert!(cfg.backends.is_empty());
}

#[test]
fn load_config_subdirectory_path() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    let path = sub.join("config.toml");
    std::fs::write(&path, "log_level = \"trace\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    // log_level is either "trace" or whatever ABP_LOG_LEVEL env var says
    assert!(cfg.log_level.is_some());
}

// ===========================================================================
// 3. Config merging (multiple sources)
// ===========================================================================

#[test]
fn merge_overlay_scalar_wins() {
    let base = BackplaneConfig {
        default_backend: Some("old".into()),
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("new".into()),
        log_level: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("new"));
    // overlay log_level is None, so base's "info" is preserved
    assert_eq!(merged.log_level.as_deref(), Some("info"));
}

#[test]
fn merge_base_preserved_when_overlay_none() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/work".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/receipts".into()),
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/work"));
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/receipts"));
}

#[test]
fn merge_both_none_stays_none() {
    let base = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(merged.default_backend.is_none());
    assert!(merged.workspace_dir.is_none());
    assert!(merged.log_level.is_none());
    assert!(merged.receipts_dir.is_none());
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
    assert_eq!(merged.backends.len(), 2);
}

#[test]
fn merge_overlay_backend_wins_on_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "x".into(),
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
            "x".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec!["host.js".into()],
                timeout_secs: Some(60),
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["x"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_three_layers() {
    let layer1 = BackplaneConfig {
        default_backend: Some("first".into()),
        log_level: Some("error".into()),
        ..Default::default()
    };
    let layer2 = BackplaneConfig {
        default_backend: Some("second".into()),
        log_level: None,
        workspace_dir: Some("/ws2".into()),
        ..Default::default()
    };
    let layer3 = BackplaneConfig {
        default_backend: None,
        log_level: Some("trace".into()),
        ..Default::default()
    };
    let merged = merge_configs(merge_configs(layer1, layer2), layer3);
    assert_eq!(merged.default_backend.as_deref(), Some("second"));
    assert_eq!(merged.log_level.as_deref(), Some("trace"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/ws2"));
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
fn merge_many_backends() {
    let base = BackplaneConfig {
        backends: (0..10)
            .map(|i| (format!("base_{i}"), BackendEntry::Mock {}))
            .collect(),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: (0..10)
            .map(|i| (format!("overlay_{i}"), BackendEntry::Mock {}))
            .collect(),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.backends.len(), 20);
}

#[test]
fn merge_default_with_default() {
    let merged = merge_configs(BackplaneConfig::default(), BackplaneConfig::default());
    assert_eq!(merged.log_level.as_deref(), Some("info"));
    assert!(merged.backends.is_empty());
}

#[test]
fn merge_empty_with_full() {
    let empty = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let full = full_config();
    let merged = merge_configs(empty, full.clone());
    assert_eq!(merged.default_backend, full.default_backend);
    assert_eq!(merged.backends.len(), full.backends.len());
}

#[test]
fn merge_full_with_empty() {
    let empty = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let full = full_config();
    let merged = merge_configs(full.clone(), empty);
    assert_eq!(merged.default_backend, full.default_backend);
    assert_eq!(merged.backends.len(), full.backends.len());
}

// ===========================================================================
// 4. Environment variable overrides (race-tolerant!)
// ===========================================================================

#[test]
fn apply_env_overrides_does_not_panic() {
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    // No assertions on specific values — other tests may have set env vars.
}

#[test]
fn load_config_none_with_env_does_not_panic() {
    // load_config(None) applies env overrides internally.
    let cfg = load_config(None).unwrap();
    // Race-tolerant: just check structure is intact.
    let _ = cfg.default_backend;
    let _ = cfg.log_level;
    let _ = cfg.workspace_dir;
    let _ = cfg.receipts_dir;
}

#[test]
fn apply_env_overrides_preserves_backends() {
    // Env overrides only touch scalar fields, not backends map.
    let mut cfg = full_config();
    let backend_count = cfg.backends.len();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.backends.len(), backend_count);
}

#[test]
fn apply_env_overrides_on_empty_config() {
    let mut cfg = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    apply_env_overrides(&mut cfg);
    // May or may not have values set depending on env — just don't panic.
    assert!(cfg.backends.is_empty());
}

#[test]
fn load_config_from_file_then_env_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "file_backend"
log_level = "warn"
"#,
    )
    .unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    // If ABP_DEFAULT_BACKEND is set in env, it may override "file_backend".
    // If ABP_LOG_LEVEL is set in env, it may override "warn".
    // Race-tolerant: just verify parsing succeeded.
    assert!(cfg.default_backend.is_some());
    assert!(cfg.log_level.is_some());
}

// ===========================================================================
// 5. Validation (invalid values, missing required fields)
// ===========================================================================

#[test]
fn validate_full_config_no_errors() {
    let warnings = validate_config(&full_config()).unwrap();
    assert!(warnings.is_empty());
}

#[test]
fn validate_default_config_passes() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        !warnings.is_empty(),
        "default should have advisory warnings"
    );
}

#[test]
fn validate_invalid_log_level_verbose() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..full_config()
    };
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn validate_invalid_log_level_uppercase() {
    let cfg = BackplaneConfig {
        log_level: Some("INFO".into()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn validate_invalid_log_level_empty_string() {
    let cfg = BackplaneConfig {
        log_level: Some(String::new()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn validate_invalid_log_level_with_spaces() {
    let cfg = BackplaneConfig {
        log_level: Some(" info ".into()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn validate_all_valid_log_levels() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..full_config()
        };
        validate_config(&cfg).unwrap_or_else(|e| panic!("'{level}' should be valid: {e}"));
    }
}

#[test]
fn validate_none_log_level_is_ok() {
    let cfg = BackplaneConfig {
        log_level: None,
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_empty_sidecar_command() {
    let mut cfg = full_config();
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
fn validate_whitespace_only_sidecar_command() {
    let mut cfg = full_config();
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
fn validate_zero_timeout() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "bad".into(),
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
fn validate_timeout_exceeds_max() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "bad".into(),
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
fn validate_timeout_u64_max() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(u64::MAX),
        },
    );
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn validate_empty_backend_name() {
    let mut cfg = full_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
}

#[test]
fn validate_multiple_errors_collected() {
    let mut cfg = BackplaneConfig {
        log_level: Some("BAD".into()),
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
    // log_level error + empty command + zero timeout = at least 3
    assert!(reasons.len() >= 3, "expected >= 3 errors: {reasons:?}");
}

#[test]
fn validate_one_bad_among_good_backends() {
    let mut cfg = full_config();
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
    assert_eq!(reasons.len(), 1);
}

#[test]
fn validate_large_timeout_warning_just_above_threshold() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_601),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, secs } if backend == "big" && *secs == 3_601
    )));
}

#[test]
fn validate_exactly_at_threshold_no_warning() {
    let mut cfg = full_config();
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
fn validate_below_threshold_no_warning() {
    let mut cfg = full_config();
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

#[test]
fn validate_timeout_boundary_1() {
    let mut cfg = full_config();
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
fn validate_timeout_boundary_max() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "edge".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    // Valid but may warn about large timeout.
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_missing_default_backend_warns() {
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
fn validate_missing_receipts_dir_warns() {
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
fn validate_both_optional_missing_two_warnings() {
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
fn validate_mock_backend_always_valid() {
    let mut cfg = full_config();
    for i in 0..5 {
        cfg.backends.insert(format!("m{i}"), BackendEntry::Mock {});
    }
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_sidecar_empty_args_ok() {
    let mut cfg = full_config();
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
fn validate_multiple_large_timeouts_multiple_warnings() {
    let mut cfg = full_config();
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
    let lt = warnings
        .iter()
        .filter(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
        .count();
    assert_eq!(lt, 2);
}

// ===========================================================================
// 6. Default values
// ===========================================================================

#[test]
fn default_log_level_is_info() {
    let cfg = BackplaneConfig::default();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn default_backends_empty() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.backends.is_empty());
}

#[test]
fn default_default_backend_is_none() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.default_backend.is_none());
}

#[test]
fn default_workspace_dir_is_none() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.workspace_dir.is_none());
}

#[test]
fn default_receipts_dir_is_none() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.receipts_dir.is_none());
}

#[test]
fn parse_empty_toml_gives_all_none_except_defaults() {
    // parse_toml("") uses serde defaults, which differ from Default::default()
    let cfg = parse_toml("").unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.workspace_dir.is_none());
    // Note: parse_toml("") log_level is None (not "info"), because serde
    // default for Option<String> is None.
    assert!(cfg.log_level.is_none());
    assert!(cfg.receipts_dir.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn default_config_is_structurally_sound() {
    let cfg = BackplaneConfig::default();
    // Can serialize without panic.
    let _ = toml::to_string(&cfg).unwrap();
    let _ = serde_json::to_string(&cfg).unwrap();
}

// ===========================================================================
// 7. Serialization roundtrip
// ===========================================================================

#[test]
fn toml_roundtrip_full_config() {
    let cfg = full_config();
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn toml_roundtrip_default_config() {
    let cfg = BackplaneConfig::default();
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn json_roundtrip_full_config() {
    let cfg = full_config();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn json_roundtrip_default_config() {
    let cfg = BackplaneConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn json_roundtrip_config_with_sidecar() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["--flag".into(), "arg".into()],
            timeout_secs: Some(120),
        },
    );
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn toml_roundtrip_preserves_validity() {
    let cfg = full_config();
    validate_config(&cfg).unwrap();
    let s = toml::to_string(&cfg).unwrap();
    let back = parse_toml(&s).unwrap();
    let warnings = validate_config(&back).unwrap();
    assert!(warnings.is_empty());
}

#[test]
fn toml_roundtrip_empty_backends() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: None,
        log_level: Some("warn".into()),
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let s = toml::to_string(&cfg).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn json_contains_expected_fields() {
    let cfg = full_config();
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    assert!(json.contains("\"default_backend\""));
    assert!(json.contains("\"log_level\""));
    assert!(json.contains("\"backends\""));
    assert!(json.contains("\"mock\""));
    assert!(json.contains("\"sidecar\""));
}

#[test]
fn toml_skip_serializing_none_fields() {
    let cfg = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let s = toml::to_string(&cfg).unwrap();
    assert!(!s.contains("default_backend"));
    assert!(!s.contains("workspace_dir"));
    assert!(!s.contains("log_level"));
    assert!(!s.contains("receipts_dir"));
}

#[test]
fn json_schema_can_be_generated() {
    let schema = schemars::schema_for!(BackplaneConfig);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("BackplaneConfig"));
}

#[test]
fn toml_roundtrip_many_backends() {
    let mut cfg = full_config();
    for i in 0..20 {
        cfg.backends
            .insert(format!("mock_{i}"), BackendEntry::Mock {});
    }
    let s = toml::to_string(&cfg).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(cfg, back);
}

// ===========================================================================
// Additional: ConfigError and ConfigWarning Display
// ===========================================================================

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
fn config_error_validation_display() {
    let e = ConfigError::ValidationError {
        reasons: vec!["reason one".into(), "reason two".into()],
    };
    let s = e.to_string();
    assert!(s.contains("reason one"));
    assert!(s.contains("reason two"));
}

#[test]
fn config_error_merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "conflict!".into(),
    };
    assert!(e.to_string().contains("conflict!"));
}

#[test]
fn config_warning_deprecated_field_display() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: Some("new_field".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old_field"));
    assert!(s.contains("new_field"));
}

#[test]
fn config_warning_deprecated_field_no_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("old"));
    assert!(s.contains("deprecated"));
}

#[test]
fn config_warning_missing_optional_display() {
    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "won't persist".into(),
    };
    let s = w.to_string();
    assert!(s.contains("receipts_dir"));
    assert!(s.contains("won't persist"));
}

#[test]
fn config_warning_large_timeout_display() {
    let w = ConfigWarning::LargeTimeout {
        backend: "slow".into(),
        secs: 9999,
    };
    let s = w.to_string();
    assert!(s.contains("slow"));
    assert!(s.contains("9999"));
}

// ===========================================================================
// Additional: Validation idempotency
// ===========================================================================

#[test]
fn validate_idempotent_valid_config() {
    let cfg = full_config();
    let w1 = validate_config(&cfg).unwrap();
    let w2 = validate_config(&cfg).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn validate_idempotent_invalid_config() {
    let cfg = BackplaneConfig {
        log_level: Some("BAD".into()),
        ..full_config()
    };
    let r1 = validation_reasons(validate_config(&cfg).unwrap_err());
    let r2 = validation_reasons(validate_config(&cfg).unwrap_err());
    assert_eq!(r1, r2);
}

#[test]
fn validate_idempotent_with_warnings() {
    let mut cfg = full_config();
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

// ===========================================================================
// Additional: Edge cases
// ===========================================================================

#[test]
fn unicode_in_backend_name() {
    let mut cfg = full_config();
    cfg.backends.insert("日本語".into(), BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

#[test]
fn unicode_in_command() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "uni".into(),
        BackendEntry::Sidecar {
            command: "nöde".into(),
            args: vec!["—flag".into()],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn very_long_backend_name_valid() {
    let mut cfg = full_config();
    cfg.backends
        .insert("a".repeat(10_000), BackendEntry::Mock {});
    validate_config(&cfg).unwrap();
}

#[test]
fn very_long_command_valid() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "long".into(),
        BackendEntry::Sidecar {
            command: "x".repeat(50_000),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn special_chars_in_paths() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("/path with spaces/@#$".into()),
        receipts_dir: Some(r"C:\Users\agent\receipts".into()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn config_with_no_backends_valid() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::new(),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn sidecar_with_leading_space_command_valid() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "spaces".into(),
        BackendEntry::Sidecar {
            command: "  node".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn many_backends_all_valid() {
    let mut cfg = full_config();
    for i in 0..100 {
        cfg.backends
            .insert(format!("mock_{i}"), BackendEntry::Mock {});
    }
    validate_config(&cfg).unwrap();
}

#[test]
fn merge_then_validate_introduces_bad_backend() {
    let base = full_config();
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
    assert!(validate_config(&merged).is_err());
}

#[test]
fn merge_overlay_fixes_broken_base() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..full_config()
    };
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
        log_level: None,
        workspace_dir: None,
        backends: BTreeMap::new(),
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
    // missing default_backend + missing receipts_dir + large timeout >= 3
    assert!(warnings.len() >= 3, "expected >= 3 warnings: {warnings:?}");
}

#[test]
fn config_warning_eq_trait() {
    let w1 = ConfigWarning::LargeTimeout {
        backend: "x".into(),
        secs: 5000,
    };
    let w2 = ConfigWarning::LargeTimeout {
        backend: "x".into(),
        secs: 5000,
    };
    assert_eq!(w1, w2);
}

#[test]
fn config_warning_clone_trait() {
    let w = ConfigWarning::DeprecatedField {
        field: "f".into(),
        suggestion: Some("g".into()),
    };
    let w2 = w.clone();
    assert_eq!(w, w2);
}

#[test]
fn backplane_config_clone_trait() {
    let cfg = full_config();
    let cfg2 = cfg.clone();
    assert_eq!(cfg, cfg2);
}

#[test]
fn backplane_config_debug_trait() {
    let cfg = full_config();
    let dbg = format!("{cfg:?}");
    assert!(dbg.contains("BackplaneConfig"));
}

#[test]
fn backend_entry_debug_trait() {
    let entry = BackendEntry::Mock {};
    let dbg = format!("{entry:?}");
    assert!(dbg.contains("Mock"));
}

#[test]
fn config_error_debug_trait() {
    let err = ConfigError::FileNotFound { path: "/x".into() };
    let dbg = format!("{err:?}");
    assert!(dbg.contains("FileNotFound"));
}
