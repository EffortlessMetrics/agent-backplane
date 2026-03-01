// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_cli::config::{
    BackendConfig, BackplaneConfig, apply_env_overrides, load_config, merge_configs,
    validate_config,
};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// 1. Load valid TOML config
// ---------------------------------------------------------------------------
#[test]
fn load_valid_toml_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "mock"
log_level = "info"

[backends.mock]
type = "mock"

[backends.openai]
type = "sidecar"
command = "node"
args = ["sidecar.js"]
"#,
    )
    .unwrap();

    let config = load_config(Some(&path)).unwrap();
    assert_eq!(config.backends.len(), 2);
    assert_eq!(config.default_backend.as_deref(), Some("mock"));
    assert_eq!(config.log_level.as_deref(), Some("info"));
    validate_config(&config).unwrap();
}

// ---------------------------------------------------------------------------
// 2. Load missing file → graceful fallback
// ---------------------------------------------------------------------------
#[test]
fn load_none_returns_defaults() {
    let config = load_config(None).unwrap();
    assert!(config.backends.is_empty());
    assert!(config.default_backend.is_none());
    assert!(config.log_level.is_none());
    assert!(config.receipts_dir.is_none());
}

// ---------------------------------------------------------------------------
// 3. Load invalid TOML → helpful error
// ---------------------------------------------------------------------------
#[test]
fn invalid_toml_gives_helpful_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(&path, "not valid [[[ toml").unwrap();

    let err = load_config(Some(&path)).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("failed to parse"), "unexpected error: {msg}");
}

// ---------------------------------------------------------------------------
// 4. Merge two configs (overlay wins)
// ---------------------------------------------------------------------------
#[test]
fn merge_overlay_wins() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("warn".into()),
        receipts_dir: Some("/tmp/base".into()),
        backends: HashMap::from([("mock".into(), BackendConfig::Mock {})]),
    };
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        log_level: None,
        receipts_dir: None,
        backends: HashMap::from([(
            "openai".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec!["sidecar.js".into()],
                timeout_secs: None,
            },
        )]),
    };

    let merged = merge_configs(base, overlay);
    // Overlay's explicit value wins.
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
    // Overlay was None → base value preserved.
    assert_eq!(merged.log_level.as_deref(), Some("warn"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/tmp/base"));
    // Both backends present.
    assert_eq!(merged.backends.len(), 2);
    assert!(merged.backends.contains_key("mock"));
    assert!(merged.backends.contains_key("openai"));
}

// ---------------------------------------------------------------------------
// 5. Env var overrides
// ---------------------------------------------------------------------------
#[test]
fn env_var_overrides() {
    let mut config = BackplaneConfig::default();

    // SAFETY: test-only; no other thread reads these vars concurrently in
    // this test binary (each integration test file is a separate process).
    unsafe {
        std::env::set_var("ABP_DEFAULT_BACKEND", "env-backend");
        std::env::set_var("ABP_LOG_LEVEL", "trace");
        std::env::set_var("ABP_RECEIPTS_DIR", "/tmp/receipts");
    }

    apply_env_overrides(&mut config);

    assert_eq!(config.default_backend.as_deref(), Some("env-backend"));
    assert_eq!(config.log_level.as_deref(), Some("trace"));
    assert_eq!(config.receipts_dir.as_deref(), Some("/tmp/receipts"));

    // Clean up.
    unsafe {
        std::env::remove_var("ABP_DEFAULT_BACKEND");
        std::env::remove_var("ABP_LOG_LEVEL");
        std::env::remove_var("ABP_RECEIPTS_DIR");
    }
}

// ---------------------------------------------------------------------------
// 6. Schema validation (via JSON-Schema)
// ---------------------------------------------------------------------------
#[test]
fn schema_validates_full_config() {
    let schema_value = {
        let schema = schemars::schema_for!(BackplaneConfig);
        serde_json::to_value(schema).unwrap()
    };
    let instance = serde_json::json!({
        "default_backend": "mock",
        "log_level": "debug",
        "receipts_dir": "./receipts",
        "backends": {
            "mock": { "type": "mock" },
            "sc": { "type": "sidecar", "command": "node", "args": ["h.js"], "timeout_secs": 60 }
        }
    });
    let validator = jsonschema::validator_for(&schema_value).expect("compile schema");
    assert!(validator.is_valid(&instance));
}

// ---------------------------------------------------------------------------
// 7. Default values
// ---------------------------------------------------------------------------
#[test]
fn default_config_is_empty_and_valid() {
    let config = BackplaneConfig::default();
    assert!(config.backends.is_empty());
    assert!(config.default_backend.is_none());
    assert!(config.log_level.is_none());
    assert!(config.receipts_dir.is_none());
    validate_config(&config).unwrap();
}

// ---------------------------------------------------------------------------
// 8. Empty config file
// ---------------------------------------------------------------------------
#[test]
fn empty_config_file_uses_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(&path, "").unwrap();

    let config = load_config(Some(&path)).unwrap();
    assert!(config.backends.is_empty());
    validate_config(&config).unwrap();
}

// ---------------------------------------------------------------------------
// 9. Config with unknown fields (should not error)
// ---------------------------------------------------------------------------
#[test]
fn unknown_fields_are_tolerated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(
        &path,
        r#"
some_future_key = true

[backends.mock]
type = "mock"
"#,
    )
    .unwrap();

    let config = load_config(Some(&path)).unwrap();
    assert_eq!(config.backends.len(), 1);
}

// ---------------------------------------------------------------------------
// 10. Config roundtrip (load → serialize → load)
// ---------------------------------------------------------------------------
#[test]
fn config_roundtrip() {
    let original = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("./data/receipts".into()),
        backends: HashMap::from([
            ("mock".into(), BackendConfig::Mock {}),
            (
                "sc".into(),
                BackendConfig::Sidecar {
                    command: "python3".into(),
                    args: vec!["host.py".into()],
                    timeout_secs: Some(120),
                },
            ),
        ]),
    };

    let toml_str = toml::to_string(&original).expect("serialize to TOML");
    let reloaded: BackplaneConfig = toml::from_str(&toml_str).expect("deserialize from TOML");

    assert_eq!(reloaded.default_backend, original.default_backend);
    assert_eq!(reloaded.log_level, original.log_level);
    assert_eq!(reloaded.receipts_dir, original.receipts_dir);
    assert_eq!(reloaded.backends.len(), original.backends.len());
}

// ---------------------------------------------------------------------------
// 11. Validate backends section
// ---------------------------------------------------------------------------
#[test]
fn validate_detects_empty_sidecar_command() {
    let config = BackplaneConfig {
        backends: HashMap::from([(
            "bad".into(),
            BackendConfig::Sidecar {
                command: "  ".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let errs = validate_config(&config).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, abp_cli::config::ConfigError::InvalidBackend { .. }))
    );
}

#[test]
fn validate_detects_excessive_timeout() {
    let config = BackplaneConfig {
        backends: HashMap::from([(
            "s".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(100_000),
            },
        )]),
        ..Default::default()
    };
    let errs = validate_config(&config).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, abp_cli::config::ConfigError::InvalidTimeout { .. }))
    );
}

// ---------------------------------------------------------------------------
// 12. Realistic config scenario
// ---------------------------------------------------------------------------
#[test]
fn realistic_config_scenario() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "sidecar:claude"
log_level = "info"
receipts_dir = "./data/receipts"

[backends.mock]
type = "mock"

[backends."sidecar:node"]
type = "sidecar"
command = "node"
args = ["hosts/node/index.js"]
timeout_secs = 300

[backends."sidecar:claude"]
type = "sidecar"
command = "node"
args = ["hosts/claude/index.js"]
timeout_secs = 600
"#,
    )
    .unwrap();

    let config = load_config(Some(&path)).unwrap();
    assert_eq!(config.default_backend.as_deref(), Some("sidecar:claude"));
    assert_eq!(config.log_level.as_deref(), Some("info"));
    assert_eq!(config.receipts_dir.as_deref(), Some("./data/receipts"));
    assert_eq!(config.backends.len(), 3);
    validate_config(&config).unwrap();
}

// ---------------------------------------------------------------------------
// 13. Merge preserves base backends not in overlay
// ---------------------------------------------------------------------------
#[test]
fn merge_preserves_base_only_backends() {
    let base = BackplaneConfig {
        backends: HashMap::from([
            ("a".into(), BackendConfig::Mock {}),
            ("b".into(), BackendConfig::Mock {}),
        ]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: HashMap::from([(
            "b".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };

    let merged = merge_configs(base, overlay);
    assert_eq!(merged.backends.len(), 2);
    // "a" preserved from base.
    assert!(matches!(merged.backends["a"], BackendConfig::Mock {}));
    // "b" overwritten by overlay.
    assert!(matches!(
        merged.backends["b"],
        BackendConfig::Sidecar { .. }
    ));
}
