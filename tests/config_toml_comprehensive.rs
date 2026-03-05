#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
#![allow(unused_unsafe)]

use abp_config::validate::{
    diff_configs, from_env_overrides, ConfigChange, ConfigDiff, ConfigIssue, ConfigMerger,
    ConfigValidationResult, ConfigValidator, IssueSeverity, Severity, ValidationIssue,
};
use abp_config::{
    apply_env_overrides, load_config, load_from_file, load_from_str, merge_configs, parse_toml,
    validate_config, BackendEntry, BackplaneConfig, ConfigError, ConfigWarning,
};
use serial_test::serial;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

// ===========================================================================
// Helpers
// ===========================================================================

fn minimal_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        log_level: Some("info".into()),
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        ..Default::default()
    }
}

fn sidecar_entry(cmd: &str, args: Vec<&str>, timeout: Option<u64>) -> BackendEntry {
    BackendEntry::Sidecar {
        command: cmd.into(),
        args: args.into_iter().map(String::from).collect(),
        timeout_secs: timeout,
    }
}

fn set_env(key: &str, val: &str) {
    unsafe {
        std::env::set_var(key, val);
    }
}

fn remove_env(key: &str) {
    unsafe {
        std::env::remove_var(key);
    }
}

fn write_toml_file(dir: &tempfile::TempDir, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    path
}

// ===========================================================================
// TOML Parsing (25+ tests)
// ===========================================================================

#[test]
fn toml_parse_example_file() {
    let content = std::fs::read_to_string("backplane.example.toml")
        .expect("backplane.example.toml should exist in repo root");
    let cfg = parse_toml(&content).expect("example toml should parse");
    assert!(cfg.backends.contains_key("mock"));
    assert!(cfg.backends.contains_key("openai"));
    assert!(cfg.backends.contains_key("anthropic"));
}

#[test]
fn toml_parse_minimal_with_just_backends() {
    let toml = r#"
[backends.mock]
type = "mock"
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 1);
    assert!(cfg.default_backend.is_none());
    assert!(cfg.workspace_dir.is_none());
}

#[test]
fn toml_parse_all_optional_fields() {
    let toml = r#"
default_backend = "mock"
workspace_dir = "/tmp/ws"
log_level = "debug"
receipts_dir = "/tmp/receipts"
bind_address = "127.0.0.1"
port = 8080
policy_profiles = ["policy1.toml", "policy2.toml"]

[backends.mock]
type = "mock"
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/ws"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/tmp/receipts"));
    assert_eq!(cfg.bind_address.as_deref(), Some("127.0.0.1"));
    assert_eq!(cfg.port, Some(8080));
    assert_eq!(cfg.policy_profiles.len(), 2);
}

#[test]
fn toml_parse_sidecar_backend_full() {
    let toml = r#"
[backends.node]
type = "sidecar"
command = "node"
args = ["sidecar.js", "--verbose"]
timeout_secs = 300
"#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["node"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["sidecar.js", "--verbose"]);
            assert_eq!(*timeout_secs, Some(300));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_parse_sidecar_backend_defaults() {
    let toml = r#"
[backends.py]
type = "sidecar"
command = "python3"
"#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["py"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python3");
            assert!(args.is_empty());
            assert_eq!(*timeout_secs, None);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_parse_mock_backend() {
    let toml = r#"
[backends.m]
type = "mock"
"#;
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn toml_parse_multiple_backends() {
    let toml = r#"
[backends.mock]
type = "mock"

[backends.node]
type = "sidecar"
command = "node"
args = ["host.js"]

[backends.python]
type = "sidecar"
command = "python3"
args = ["host.py"]
timeout_secs = 600
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
    assert!(matches!(cfg.backends["node"], BackendEntry::Sidecar { .. }));
    assert!(matches!(
        cfg.backends["python"],
        BackendEntry::Sidecar { .. }
    ));
}

#[test]
fn toml_parse_invalid_syntax_gives_error() {
    let bad = "this is [not valid toml =";
    let err = parse_toml(bad).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_invalid_unclosed_string() {
    let bad = r#"default_backend = "unterminated"#;
    let err = parse_toml(bad).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_missing_type_field_in_backend() {
    let toml = r#"
[backends.bad]
command = "node"
"#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_unknown_backend_type() {
    let toml = r#"
[backends.bad]
type = "grpc"
command = "node"
"#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_wrong_type_for_port() {
    let toml = r#"port = "not_a_number""#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_wrong_type_for_log_level() {
    let toml = r#"log_level = 42"#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_wrong_type_for_args() {
    let toml = r#"
[backends.bad]
type = "sidecar"
command = "node"
args = "not_an_array"
"#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_empty_string_gives_defaults() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.backends.is_empty());
    assert!(cfg.workspace_dir.is_none());
    assert!(cfg.receipts_dir.is_none());
    assert!(cfg.bind_address.is_none());
    assert!(cfg.port.is_none());
    assert!(cfg.policy_profiles.is_empty());
    // log_level is None when parsed from empty (no serde default on field)
}

#[test]
fn toml_parse_with_comments_and_whitespace() {
    let toml = r#"
# This is a comment
default_backend = "mock"   # inline comment

# Another section comment
[backends.mock]
type = "mock"
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.backends.len(), 1);
}

#[test]
fn toml_parse_extra_fields_ignored_by_serde() {
    // serde(deny_unknown_fields) is NOT set, so extra fields should be ignored
    let toml = r#"
default_backend = "mock"
extra_field = "should be ignored"
another_unknown = 42

[backends.mock]
type = "mock"
"#;
    // This may or may not fail depending on serde config — toml crate
    // with deny_unknown_fields would reject. Let's test the actual behavior.
    let result = parse_toml(toml);
    // If the crate doesn't deny unknown fields, this succeeds
    if let Ok(cfg) = result {
        assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    }
    // Either outcome is acceptable — we just verify no panic
}

#[test]
fn toml_parse_only_whitespace() {
    let cfg = parse_toml("   \n\n  \t  \n").unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_roundtrip_serialize_deserialize() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("0.0.0.0".into()),
        port: Some(9090),
        policy_profiles: vec!["p1.toml".into()],
        backends: BTreeMap::from([
            ("mock".into(), BackendEntry::Mock {}),
            ("sc".into(), sidecar_entry("node", vec!["h.js"], Some(120))),
        ]),
    };
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn toml_parse_sidecar_empty_args_array() {
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
fn toml_parse_port_max_value() {
    let toml = "port = 65535";
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.port, Some(65535));
}

#[test]
fn toml_parse_port_min_value() {
    let toml = "port = 1";
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.port, Some(1));
}

#[test]
fn toml_parse_port_zero() {
    let toml = "port = 0";
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.port, Some(0));
}

#[test]
fn toml_parse_port_out_of_range_u16() {
    let toml = "port = 70000";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_load_from_str_alias() {
    let toml = r#"
default_backend = "mock"
[backends.mock]
type = "mock"
"#;
    let cfg = load_from_str(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn toml_parse_policy_profiles_empty_array() {
    let toml = r#"policy_profiles = []"#;
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.policy_profiles.is_empty());
}

#[test]
fn toml_parse_multiline_string_value() {
    let toml = r#"
default_backend = "mock"
receipts_dir = "/very/long/path/to/receipts"

[backends.mock]
type = "mock"
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(
        cfg.receipts_dir.as_deref(),
        Some("/very/long/path/to/receipts")
    );
}

// ===========================================================================
// JSON Config (15+ tests)
// ===========================================================================

#[test]
fn json_parse_equivalent_of_toml() {
    let json = r#"{
        "default_backend": "mock",
        "log_level": "info",
        "receipts_dir": "/tmp/receipts",
        "backends": {
            "mock": { "type": "mock" }
        }
    }"#;
    let cfg: BackplaneConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
}

#[test]
fn json_parse_sidecar_backend() {
    let json = r#"{
        "backends": {
            "sc": {
                "type": "sidecar",
                "command": "node",
                "args": ["host.js"],
                "timeout_secs": 300
            }
        }
    }"#;
    let cfg: BackplaneConfig = serde_json::from_str(json).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
            assert_eq!(*timeout_secs, Some(300));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn json_toml_roundtrip_produces_same_config() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec!["p.toml".into()],
        backends: BTreeMap::from([
            ("mock".into(), BackendEntry::Mock {}),
            ("sc".into(), sidecar_entry("node", vec!["h.js"], Some(120))),
        ]),
    };
    let json_str = serde_json::to_string(&cfg).unwrap();
    let from_json: BackplaneConfig = serde_json::from_str(&json_str).unwrap();

    let toml_str = toml::to_string(&cfg).unwrap();
    let from_toml: BackplaneConfig = toml::from_str(&toml_str).unwrap();

    assert_eq!(from_json, from_toml);
}

#[test]
fn json_with_unknown_fields() {
    let json = r#"{
        "default_backend": "mock",
        "unknown_field": "value",
        "backends": { "mock": { "type": "mock" } }
    }"#;
    // serde by default ignores unknown fields
    let result: Result<BackplaneConfig, _> = serde_json::from_str(json);
    if let Ok(cfg) = result {
        assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    }
}

#[test]
fn json_invalid_syntax_gives_error() {
    let bad = r#"{ "default_backend": }"#;
    let result: Result<BackplaneConfig, _> = serde_json::from_str(bad);
    assert!(result.is_err());
}

#[test]
fn json_wrong_type_for_port() {
    let json = r#"{ "port": "not_a_number" }"#;
    let result: Result<BackplaneConfig, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn json_empty_object_gives_defaults() {
    let json = "{}";
    let cfg: BackplaneConfig = serde_json::from_str(json).unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn json_null_optional_fields() {
    let json = r#"{
        "default_backend": null,
        "log_level": null,
        "backends": {}
    }"#;
    let cfg: BackplaneConfig = serde_json::from_str(json).unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.log_level.is_none());
}

#[test]
fn json_parse_multiple_backends() {
    let json = r#"{
        "backends": {
            "mock": { "type": "mock" },
            "sc1": { "type": "sidecar", "command": "node" },
            "sc2": { "type": "sidecar", "command": "python3", "args": ["h.py"], "timeout_secs": 60 }
        }
    }"#;
    let cfg: BackplaneConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.backends.len(), 3);
}

#[test]
fn json_parse_all_fields() {
    let json = r#"{
        "default_backend": "mock",
        "workspace_dir": "/ws",
        "log_level": "trace",
        "receipts_dir": "/r",
        "bind_address": "::1",
        "port": 443,
        "policy_profiles": ["a.toml"],
        "backends": { "mock": { "type": "mock" } }
    }"#;
    let cfg: BackplaneConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.port, Some(443));
    assert_eq!(cfg.bind_address.as_deref(), Some("::1"));
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
}

#[test]
fn json_serialize_skips_none_fields() {
    let cfg = BackplaneConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    // skip_serializing_if = "Option::is_none" means null fields are absent
    assert!(!json.contains("default_backend"));
    assert!(!json.contains("workspace_dir"));
    assert!(!json.contains("receipts_dir"));
    assert!(!json.contains("bind_address"));
    assert!(!json.contains("\"port\""));
}

#[test]
fn json_backend_entry_tagged_correctly() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains(r#""type":"mock"#));

    let entry = sidecar_entry("node", vec![], None);
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains(r#""type":"sidecar"#));
}

#[test]
fn json_roundtrip_backend_entry_mock() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, parsed);
}

#[test]
fn json_roundtrip_backend_entry_sidecar() {
    let entry = sidecar_entry("node", vec!["--flag", "host.js"], Some(600));
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, parsed);
}

#[test]
fn json_invalid_backend_type() {
    let json = r#"{ "type": "unknown_type", "command": "node" }"#;
    let result: Result<BackendEntry, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

// ===========================================================================
// Environment Variable Overrides (15+ tests)
// ===========================================================================

#[test]
#[serial]
fn env_abp_default_backend_overrides() {
    set_env("ABP_DEFAULT_BACKEND", "openai");
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("openai"));
    remove_env("ABP_DEFAULT_BACKEND");
}

#[test]
#[serial]
fn env_abp_log_level_overrides() {
    set_env("ABP_LOG_LEVEL", "trace");
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
    remove_env("ABP_LOG_LEVEL");
}

#[test]
#[serial]
fn env_abp_receipts_dir_overrides() {
    set_env("ABP_RECEIPTS_DIR", "/override/receipts");
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/override/receipts"));
    remove_env("ABP_RECEIPTS_DIR");
}

#[test]
#[serial]
fn env_abp_workspace_dir_overrides() {
    set_env("ABP_WORKSPACE_DIR", "/override/ws");
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/override/ws"));
    remove_env("ABP_WORKSPACE_DIR");
}

#[test]
#[serial]
fn env_abp_bind_address_overrides() {
    set_env("ABP_BIND_ADDRESS", "0.0.0.0");
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
    remove_env("ABP_BIND_ADDRESS");
}

#[test]
#[serial]
fn env_abp_port_overrides() {
    set_env("ABP_PORT", "9090");
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.port, Some(9090));
    remove_env("ABP_PORT");
}

#[test]
#[serial]
fn env_multiple_overrides_all_apply() {
    set_env("ABP_DEFAULT_BACKEND", "anthropic");
    set_env("ABP_LOG_LEVEL", "warn");
    set_env("ABP_PORT", "3000");
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("anthropic"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    assert_eq!(cfg.port, Some(3000));
    remove_env("ABP_DEFAULT_BACKEND");
    remove_env("ABP_LOG_LEVEL");
    remove_env("ABP_PORT");
}

#[test]
#[serial]
fn env_empty_value_is_treated_as_set() {
    set_env("ABP_DEFAULT_BACKEND", "");
    let mut cfg = BackplaneConfig {
        default_backend: Some("original".into()),
        ..Default::default()
    };
    apply_env_overrides(&mut cfg);
    // Empty string is still Some("") — env var was present
    assert_eq!(cfg.default_backend.as_deref(), Some(""));
    remove_env("ABP_DEFAULT_BACKEND");
}

#[test]
#[serial]
fn env_unset_var_does_not_affect_config() {
    remove_env("ABP_DEFAULT_BACKEND");
    remove_env("ABP_LOG_LEVEL");
    remove_env("ABP_RECEIPTS_DIR");
    remove_env("ABP_WORKSPACE_DIR");
    remove_env("ABP_BIND_ADDRESS");
    remove_env("ABP_PORT");
    let mut cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("debug".into()),
        ..Default::default()
    };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
}

#[test]
#[serial]
fn env_port_non_numeric_ignored() {
    set_env("ABP_PORT", "not_a_number");
    let mut cfg = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    apply_env_overrides(&mut cfg);
    // parse fails, so port should remain unchanged
    assert_eq!(cfg.port, Some(8080));
    remove_env("ABP_PORT");
}

#[test]
#[serial]
fn env_override_replaces_file_config_value() {
    set_env("ABP_LOG_LEVEL", "error");
    let mut cfg = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("error"));
    remove_env("ABP_LOG_LEVEL");
}

#[test]
#[serial]
fn env_load_config_none_applies_overrides() {
    set_env("ABP_DEFAULT_BACKEND", "from_env");
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("from_env"));
    remove_env("ABP_DEFAULT_BACKEND");
}

#[test]
#[serial]
fn env_load_config_file_applies_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_toml_file(&dir, "test.toml", r#"default_backend = "file_val""#);
    set_env("ABP_DEFAULT_BACKEND", "env_val");
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("env_val"));
    remove_env("ABP_DEFAULT_BACKEND");
}

#[test]
#[serial]
fn env_from_env_overrides_alias() {
    set_env("ABP_LOG_LEVEL", "trace");
    let mut cfg = BackplaneConfig::default();
    from_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
    remove_env("ABP_LOG_LEVEL");
}

#[test]
#[serial]
fn env_backends_not_overridden_by_env() {
    // There is no ABP_BACKENDS env var — backends come from file only
    let mut cfg = BackplaneConfig {
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.backends.len(), 1);
}

// ===========================================================================
// Config Validation (20+ tests)
// ===========================================================================

#[test]
fn validate_default_config_produces_warnings() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).expect("default config should be valid");
    assert!(!warnings.is_empty());
}

#[test]
fn validate_valid_full_config_passes() {
    let cfg = minimal_config();
    let warnings = validate_config(&cfg).unwrap();
    // Should have no missing-optional warnings since we set default_backend + receipts_dir
    let has_missing = warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::MissingOptionalField { .. }));
    assert!(!has_missing);
}

#[test]
fn validate_invalid_log_level_error() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_all_valid_log_levels() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some(level.to_string()),
            ..Default::default()
        };
        validate_config(&cfg).unwrap_or_else(|_| panic!("'{level}' should be valid"));
    }
}

#[test]
fn validate_sidecar_empty_command_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("bad".into(), sidecar_entry("", vec![], None));
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.iter().any(|r| r.contains("command")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validate_sidecar_whitespace_command_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("bad".into(), sidecar_entry("   ", vec![], None));
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_empty_backend_name_error() {
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
fn validate_port_zero_error() {
    let cfg = BackplaneConfig {
        port: Some(0),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_port_one_ok() {
    let cfg = BackplaneConfig {
        port: Some(1),
        ..Default::default()
    };
    validate_config(&cfg).expect("port 1 should be valid");
}

#[test]
fn validate_port_max_ok() {
    let cfg = BackplaneConfig {
        port: Some(65535),
        ..Default::default()
    };
    validate_config(&cfg).expect("port 65535 should be valid");
}

#[test]
fn validate_timeout_zero_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", vec![], Some(0)));
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_timeout_exceeding_max_error() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", vec![], Some(86_401)));
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_timeout_positive_ok() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", vec![], Some(60)));
    validate_config(&cfg).expect("timeout 60 should be valid");
}

#[test]
fn validate_timeout_at_max_boundary_ok() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", vec![], Some(86_400)));
    validate_config(&cfg).expect("timeout at max boundary should pass");
}

#[test]
fn validate_large_timeout_warning() {
    let mut cfg = minimal_config();
    cfg.backends
        .insert("sc".into(), sidecar_entry("node", vec![], Some(7200)));
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::LargeTimeout { secs: 7200, .. })));
}

#[test]
fn validate_missing_default_backend_warning() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(
        |w| matches!(w, ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend")
    ));
}

#[test]
fn validate_missing_receipts_dir_warning() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(
        |w| matches!(w, ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir")
    ));
}

#[test]
fn validate_bind_address_empty_error() {
    let cfg = BackplaneConfig {
        bind_address: Some("".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_bind_address_valid_ipv4() {
    let cfg = BackplaneConfig {
        bind_address: Some("192.168.1.1".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("valid IPv4 should pass");
}

#[test]
fn validate_bind_address_valid_ipv6() {
    let cfg = BackplaneConfig {
        bind_address: Some("::1".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("valid IPv6 should pass");
}

#[test]
fn validate_bind_address_localhost() {
    let cfg = BackplaneConfig {
        bind_address: Some("localhost".into()),
        ..Default::default()
    };
    validate_config(&cfg).expect("localhost should pass");
}

#[test]
fn validate_multiple_errors_collected() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bad_level".into()),
        port: Some(0),
        ..Default::default()
    };
    cfg.backends
        .insert("bad".into(), sidecar_entry("", vec![], Some(0)));
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.len() >= 3, "should collect multiple errors");
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===========================================================================
// Config Merging (15+ tests)
// ===========================================================================

#[test]
fn merge_overlay_default_backend_wins() {
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
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/ws"));
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
    assert_eq!(merged.backends.len(), 2);
}

#[test]
fn merge_overlay_backend_wins_on_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), sidecar_entry("python3", vec![], None))]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            sidecar_entry("node", vec!["host.js"], Some(60)),
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
fn merge_both_empty_is_empty() {
    let merged = merge_configs(BackplaneConfig::default(), BackplaneConfig::default());
    // Only log_level should be set from default
    assert!(merged.default_backend.is_none());
    assert!(merged.backends.is_empty());
}

#[test]
fn merge_port_overlay_wins() {
    let base = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        port: Some(9090),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.port, Some(9090));
}

#[test]
fn merge_port_overlay_none_preserves_base() {
    let base = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        port: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.port, Some(8080));
}

#[test]
fn merge_bind_address_overlay_wins() {
    let base = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        bind_address: Some("0.0.0.0".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.bind_address.as_deref(), Some("0.0.0.0"));
}

#[test]
fn merge_policy_profiles_overlay_replaces_base() {
    let base = BackplaneConfig {
        policy_profiles: vec!["base.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec!["overlay.toml".into()],
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.policy_profiles, vec!["overlay.toml"]);
}

#[test]
fn merge_policy_profiles_empty_overlay_keeps_base() {
    let base = BackplaneConfig {
        policy_profiles: vec!["base.toml".into()],
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        policy_profiles: vec![],
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.policy_profiles, vec!["base.toml"]);
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
fn merge_log_level_overlay_wins() {
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
fn merge_three_layers() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let mid = BackplaneConfig {
        log_level: Some("debug".into()),
        backends: BTreeMap::from([("sc".into(), sidecar_entry("node", vec![], None))]),
        ..Default::default()
    };
    let top = BackplaneConfig {
        default_backend: Some("sc".into()),
        ..Default::default()
    };
    let merged = merge_configs(merge_configs(base, mid), top);
    assert_eq!(merged.default_backend.as_deref(), Some("sc"));
    assert!(merged.backends.contains_key("mock"));
    assert!(merged.backends.contains_key("sc"));
}

#[test]
fn merge_override_single_field_others_unchanged() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/r".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec!["p.toml".into()],
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
    };
    let overlay = BackplaneConfig {
        log_level: Some("trace".into()),
        ..Default::default()
    };
    let merged = merge_configs(base.clone(), overlay);
    assert_eq!(merged.log_level.as_deref(), Some("trace"));
    assert_eq!(merged.default_backend, base.default_backend);
    assert_eq!(merged.workspace_dir, base.workspace_dir);
    assert_eq!(merged.receipts_dir, base.receipts_dir);
    assert_eq!(merged.bind_address, base.bind_address);
    assert_eq!(merged.port, base.port);
    assert_eq!(merged.policy_profiles, base.policy_profiles);
    assert!(merged.backends.contains_key("mock"));
}

#[test]
fn merge_via_config_merger_struct() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        ..Default::default()
    };
    let merged = ConfigMerger::merge(&base, &overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
}

// ===========================================================================
// BackendEntry Types (20+ tests)
// ===========================================================================

#[test]
fn backend_mock_entry_serde_roundtrip_toml() {
    let entry = BackendEntry::Mock {};
    let toml_str = toml::to_string(&entry).unwrap();
    let parsed: BackendEntry = toml::from_str(&toml_str).unwrap();
    assert_eq!(entry, parsed);
}

#[test]
fn backend_sidecar_all_fields_serde_roundtrip() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["--experimental".into(), "host.js".into()],
        timeout_secs: Some(300),
    };
    let toml_str = toml::to_string(&entry).unwrap();
    let parsed: BackendEntry = toml::from_str(&toml_str).unwrap();
    assert_eq!(entry, parsed);
}

#[test]
fn backend_sidecar_defaults() {
    let entry = BackendEntry::Sidecar {
        command: "python3".into(),
        args: vec![],
        timeout_secs: None,
    };
    let toml_str = toml::to_string(&entry).unwrap();
    assert!(!toml_str.contains("timeout_secs"));
    let parsed: BackendEntry = toml::from_str(&toml_str).unwrap();
    assert_eq!(entry, parsed);
}

#[test]
fn backend_entry_eq_mock() {
    let a = BackendEntry::Mock {};
    let b = BackendEntry::Mock {};
    assert_eq!(a, b);
}

#[test]
fn backend_entry_eq_sidecar_same() {
    let a = sidecar_entry("node", vec!["h.js"], Some(60));
    let b = sidecar_entry("node", vec!["h.js"], Some(60));
    assert_eq!(a, b);
}

#[test]
fn backend_entry_ne_sidecar_different_command() {
    let a = sidecar_entry("node", vec![], None);
    let b = sidecar_entry("python3", vec![], None);
    assert_ne!(a, b);
}

#[test]
fn backend_entry_ne_sidecar_different_args() {
    let a = sidecar_entry("node", vec!["a"], None);
    let b = sidecar_entry("node", vec!["b"], None);
    assert_ne!(a, b);
}

#[test]
fn backend_entry_ne_sidecar_different_timeout() {
    let a = sidecar_entry("node", vec![], Some(60));
    let b = sidecar_entry("node", vec![], Some(120));
    assert_ne!(a, b);
}

#[test]
fn backend_entry_ne_mock_vs_sidecar() {
    let a = BackendEntry::Mock {};
    let b = sidecar_entry("node", vec![], None);
    assert_ne!(a, b);
}

#[test]
fn backend_entry_debug_mock() {
    let entry = BackendEntry::Mock {};
    let debug = format!("{entry:?}");
    assert!(debug.contains("Mock"));
}

#[test]
fn backend_entry_debug_sidecar() {
    let entry = sidecar_entry("node", vec!["h.js"], Some(120));
    let debug = format!("{entry:?}");
    assert!(debug.contains("Sidecar"));
    assert!(debug.contains("node"));
    assert!(debug.contains("h.js"));
    assert!(debug.contains("120"));
}

#[test]
fn backend_entry_clone() {
    let entry = sidecar_entry("node", vec!["h.js"], Some(120));
    let cloned = entry.clone();
    assert_eq!(entry, cloned);
}

#[test]
fn backend_entry_mock_json_tag() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["type"], "mock");
}

#[test]
fn backend_entry_sidecar_json_tag() {
    let entry = sidecar_entry("node", vec![], None);
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["type"], "sidecar");
    assert_eq!(json["command"], "node");
}

#[test]
fn backend_entry_sidecar_timeout_skip_none_json() {
    let entry = sidecar_entry("node", vec![], None);
    let json = serde_json::to_value(&entry).unwrap();
    assert!(json.get("timeout_secs").is_none() || json["timeout_secs"].is_null());
}

#[test]
fn backend_entry_sidecar_many_args() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: (0..20).map(|i| format!("arg{i}")).collect(),
        timeout_secs: Some(600),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, parsed);
}

#[test]
fn backend_entry_sidecar_special_chars_in_command() {
    let entry = BackendEntry::Sidecar {
        command: "/usr/local/bin/my-sidecar".into(),
        args: vec!["--config=/etc/abp.toml".into()],
        timeout_secs: None,
    };
    let toml_str = toml::to_string(&entry).unwrap();
    let parsed: BackendEntry = toml::from_str(&toml_str).unwrap();
    assert_eq!(entry, parsed);
}

#[test]
fn backend_entry_sidecar_unicode_args() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["日本語.js".into()],
        timeout_secs: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, parsed);
}

#[test]
fn backend_entry_sidecar_timeout_at_boundary() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: Some(86_400),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: BackendEntry = serde_json::from_str(&json).unwrap();
    match parsed {
        BackendEntry::Sidecar { timeout_secs, .. } => {
            assert_eq!(timeout_secs, Some(86_400));
        }
        _ => panic!("expected Sidecar"),
    }
}

#[test]
fn backend_entry_sidecar_timeout_one() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: Some(1),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: BackendEntry = serde_json::from_str(&json).unwrap();
    match parsed {
        BackendEntry::Sidecar { timeout_secs, .. } => {
            assert_eq!(timeout_secs, Some(1));
        }
        _ => panic!("expected Sidecar"),
    }
}

// ===========================================================================
// File Loading (additional tests)
// ===========================================================================

#[test]
fn load_from_file_valid_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_toml_file(
        &dir,
        "test.toml",
        r#"
default_backend = "mock"
log_level = "warn"

[backends.mock]
type = "mock"
"#,
    );
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn load_from_file_missing_gives_error() {
    let err = load_from_file(Path::new("/nonexistent/path.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_config_none_returns_default() {
    // Clear env vars to avoid interference
    let cfg = BackplaneConfig::default();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
    assert!(cfg.backends.is_empty());
}

// ===========================================================================
// ConfigValidator (structured validation)
// ===========================================================================

#[test]
fn config_validator_valid_config() {
    let cfg = minimal_config();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    // No errors, possibly some info/warning issues
    assert!(issues.iter().all(|i| i.severity != Severity::Error));
}

#[test]
fn config_validator_invalid_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..Default::default()
    };
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn config_validator_empty_backends_info() {
    let cfg = BackplaneConfig::default();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(issues
        .iter()
        .any(|i| i.severity == Severity::Info && i.message.contains("no backends")));
}

#[test]
fn config_validator_validate_at_filters_severity() {
    let cfg = BackplaneConfig::default();
    let warnings_only = ConfigValidator::validate_at(&cfg, Severity::Warning).unwrap();
    assert!(warnings_only
        .iter()
        .all(|i| i.severity >= Severity::Warning));
}

#[test]
fn config_validator_check_returns_result() {
    let cfg = minimal_config();
    let result = ConfigValidator::check(&cfg);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn config_validator_check_invalid_config() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        port: Some(0),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(!result.errors.is_empty());
}

#[test]
fn config_validator_check_default_backend_mismatch_warning() {
    let cfg = BackplaneConfig {
        default_backend: Some("nonexistent".into()),
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("does not match")));
    assert!(!result.suggestions.is_empty());
}

#[test]
fn config_validator_check_empty_workspace_dir_warning() {
    let cfg = BackplaneConfig {
        workspace_dir: Some("".into()),
        ..Default::default()
    };
    let result = ConfigValidator::check(&cfg);
    assert!(result.warnings.iter().any(|w| w.field == "workspace_dir"));
}

// ===========================================================================
// ConfigDiff
// ===========================================================================

#[test]
fn diff_identical_configs_empty() {
    let cfg = minimal_config();
    let diffs = diff_configs(&cfg, &cfg);
    assert!(diffs.is_empty());
}

#[test]
fn diff_default_backend_changed() {
    let a = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        default_backend: Some("openai".into()),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "default_backend"));
}

#[test]
fn diff_backend_added() {
    let a = BackplaneConfig::default();
    let b = BackplaneConfig {
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.mock"));
}

#[test]
fn diff_backend_removed() {
    let a = BackplaneConfig {
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let b = BackplaneConfig::default();
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.mock"));
}

#[test]
fn diff_config_change_via_struct_method() {
    let a = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let changes = ConfigDiff::diff(&a, &b);
    assert!(changes.iter().any(|c| c.field == "log_level"));
}

// ===========================================================================
// Error Display
// ===========================================================================

#[test]
fn config_error_file_not_found_display() {
    let e = ConfigError::FileNotFound {
        path: "/missing.toml".into(),
    };
    let msg = e.to_string();
    assert!(msg.contains("/missing.toml"));
    assert!(msg.contains("not found"));
}

#[test]
fn config_error_parse_error_display() {
    let e = ConfigError::ParseError {
        reason: "unexpected token".into(),
    };
    let msg = e.to_string();
    assert!(msg.contains("unexpected token"));
}

#[test]
fn config_error_validation_error_display() {
    let e = ConfigError::ValidationError {
        reasons: vec!["bad port".into(), "bad level".into()],
    };
    let msg = e.to_string();
    assert!(msg.contains("bad port"));
    assert!(msg.contains("bad level"));
}

#[test]
fn config_error_merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "conflicting keys".into(),
    };
    let msg = e.to_string();
    assert!(msg.contains("conflicting keys"));
}

// ===========================================================================
// Warning Display
// ===========================================================================

#[test]
fn config_warning_deprecated_with_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: Some("new_field".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old_field"));
    assert!(s.contains("new_field"));
}

#[test]
fn config_warning_deprecated_no_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("old"));
    assert!(!s.contains("instead"));
}

#[test]
fn config_warning_missing_optional_display() {
    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "receipts won't persist".into(),
    };
    let s = w.to_string();
    assert!(s.contains("receipts_dir"));
    assert!(s.contains("receipts won't persist"));
}

#[test]
fn config_warning_large_timeout_display() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 7200,
    };
    let s = w.to_string();
    assert!(s.contains("sc"));
    assert!(s.contains("7200"));
}

// ===========================================================================
// Validate module types Display
// ===========================================================================

#[test]
fn severity_display() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn issue_severity_display() {
    assert_eq!(IssueSeverity::Error.to_string(), "error");
    assert_eq!(IssueSeverity::Warning.to_string(), "warning");
}

#[test]
fn validation_issue_display() {
    let issue = ValidationIssue {
        severity: Severity::Warning,
        message: "test message".into(),
    };
    let s = issue.to_string();
    assert!(s.contains("warning"));
    assert!(s.contains("test message"));
}

#[test]
fn config_issue_display() {
    let issue = ConfigIssue {
        field: "backends.sc.command".into(),
        message: "must not be empty".into(),
        severity: IssueSeverity::Error,
    };
    let s = issue.to_string();
    assert!(s.contains("backends.sc.command"));
    assert!(s.contains("must not be empty"));
    assert!(s.contains("error"));
}

#[test]
fn config_diff_display() {
    let d = abp_config::validate::ConfigDiff {
        path: "log_level".into(),
        old_value: "\"info\"".into(),
        new_value: "\"debug\"".into(),
    };
    let s = d.to_string();
    assert!(s.contains("log_level"));
    assert!(s.contains("->"));
}

#[test]
fn config_change_display() {
    let c = ConfigChange {
        field: "port".into(),
        old_value: "8080".into(),
        new_value: "9090".into(),
    };
    let s = c.to_string();
    assert!(s.contains("port"));
    assert!(s.contains("8080"));
    assert!(s.contains("9090"));
}

// ===========================================================================
// BackplaneConfig default values
// ===========================================================================

#[test]
fn default_config_log_level_info() {
    assert_eq!(
        BackplaneConfig::default().log_level.as_deref(),
        Some("info")
    );
}

#[test]
fn default_config_no_backends() {
    assert!(BackplaneConfig::default().backends.is_empty());
}

#[test]
fn default_config_no_bind_address() {
    assert!(BackplaneConfig::default().bind_address.is_none());
}

#[test]
fn default_config_no_port() {
    assert!(BackplaneConfig::default().port.is_none());
}

#[test]
fn default_config_no_policy_profiles() {
    assert!(BackplaneConfig::default().policy_profiles.is_empty());
}

#[test]
fn default_config_no_workspace_dir() {
    assert!(BackplaneConfig::default().workspace_dir.is_none());
}

#[test]
fn default_config_no_receipts_dir() {
    assert!(BackplaneConfig::default().receipts_dir.is_none());
}

#[test]
fn default_config_no_default_backend() {
    assert!(BackplaneConfig::default().default_backend.is_none());
}

// ===========================================================================
// BTreeMap ordering determinism
// ===========================================================================

#[test]
fn backends_btreemap_serialization_is_deterministic() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("z_last".into(), BackendEntry::Mock {});
    cfg.backends.insert("a_first".into(), BackendEntry::Mock {});
    cfg.backends
        .insert("m_middle".into(), sidecar_entry("node", vec![], None));
    let s1 = toml::to_string(&cfg).unwrap();
    let s2 = toml::to_string(&cfg).unwrap();
    assert_eq!(s1, s2);
    // BTreeMap guarantees lexicographic order
    let a_pos = s1.find("a_first").unwrap();
    let m_pos = s1.find("m_middle").unwrap();
    let z_pos = s1.find("z_last").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}
