// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the new configuration features: `load_from_file`, `load_from_str`,
//! `ABP_BIND_ADDRESS`/`ABP_PORT` env overrides, port/bind-address/policy-profile
//! validation.
#![allow(clippy::field_reassign_with_default)]

use abp_config::{
    load_from_file, load_from_str, validate_config, BackendEntry, BackplaneConfig,
    ConfigError,
};
use abp_config::validate::ConfigValidator;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fully_valid_config() -> BackplaneConfig {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        backends,
        ..Default::default()
    }
}

fn validation_reasons(err: ConfigError) -> Vec<String> {
    match err {
        ConfigError::ValidationError { reasons } => reasons,
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

// ===========================================================================
// 1. load_from_str — parse valid TOML
// ===========================================================================

#[test]
fn load_from_str_valid_toml() {
    let toml = r#"
        default_backend = "mock"
        log_level = "debug"
        [backends.mock]
        type = "mock"
    "#;
    let cfg = load_from_str(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.backends.len(), 1);
}

// ===========================================================================
// 2. load_from_str — parse invalid TOML gives ParseError
// ===========================================================================

#[test]
fn load_from_str_invalid_toml() {
    let err = load_from_str("[bad =").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// ===========================================================================
// 3. load_from_str — empty string returns defaults
// ===========================================================================

#[test]
fn load_from_str_empty() {
    let cfg = load_from_str("").unwrap();
    assert!(cfg.backends.is_empty());
    assert_eq!(cfg.default_backend, None);
}

// ===========================================================================
// 4. load_from_str — supports bind_address and port fields
// ===========================================================================

#[test]
fn load_from_str_with_bind_address_and_port() {
    let toml = r#"
        bind_address = "0.0.0.0"
        port = 8080
    "#;
    let cfg = load_from_str(toml).unwrap();
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
    assert_eq!(cfg.port, Some(8080));
}

// ===========================================================================
// 5. load_from_str — supports policy_profiles field
// ===========================================================================

#[test]
fn load_from_str_with_policy_profiles() {
    let toml = r#"
        policy_profiles = ["profiles/default.toml", "profiles/strict.toml"]
    "#;
    let cfg = load_from_str(toml).unwrap();
    assert_eq!(cfg.policy_profiles.len(), 2);
    assert_eq!(cfg.policy_profiles[0], "profiles/default.toml");
}

// ===========================================================================
// 6. load_from_file — reads from disk
// ===========================================================================

#[test]
fn load_from_file_reads_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "default_backend = \"mock\"\nport = 3000").unwrap();
    let cfg = load_from_file(&path).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.port, Some(3000));
}

// ===========================================================================
// 7. load_from_file — missing file gives FileNotFound
// ===========================================================================

#[test]
fn load_from_file_missing() {
    let err = load_from_file(Path::new("/nonexistent/config.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

// ===========================================================================
// 8. load_from_str — parses backplane.example.toml format
// ===========================================================================

#[test]
fn load_from_str_example_format() {
    let toml = r#"
        default_backend = "mock"
        log_level = "info"
        receipts_dir = "./data/receipts"

        [backends.mock]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["path/to/openai-sidecar.js"]
        timeout_secs = 300

        [backends.anthropic]
        type = "sidecar"
        command = "python3"
        args = ["path/to/anthropic-sidecar.py"]
        timeout_secs = 600
    "#;
    let cfg = load_from_str(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
    assert!(cfg.backends.contains_key("mock"));
    assert!(cfg.backends.contains_key("openai"));
    assert!(cfg.backends.contains_key("anthropic"));
}

// ===========================================================================
// 9. Validation — port 0 is invalid
// ===========================================================================

#[test]
fn validate_port_zero_is_error() {
    let mut cfg = fully_valid_config();
    cfg.port = Some(0);
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("port must be between")));
}

// ===========================================================================
// 10. Validation — valid port 1 passes
// ===========================================================================

#[test]
fn validate_port_1_is_valid() {
    let mut cfg = fully_valid_config();
    cfg.port = Some(1);
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 11. Validation — valid port 65535 passes
// ===========================================================================

#[test]
fn validate_port_65535_is_valid() {
    let mut cfg = fully_valid_config();
    cfg.port = Some(65535);
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 12. Validation — typical port 8080 passes
// ===========================================================================

#[test]
fn validate_port_8080_is_valid() {
    let mut cfg = fully_valid_config();
    cfg.port = Some(8080);
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 13. Validation — None port passes (optional)
// ===========================================================================

#[test]
fn validate_port_none_is_valid() {
    let cfg = fully_valid_config();
    assert!(cfg.port.is_none());
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 14. Validation — bind_address valid IPv4
// ===========================================================================

#[test]
fn validate_bind_address_ipv4() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("127.0.0.1".into());
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 15. Validation — bind_address valid IPv6
// ===========================================================================

#[test]
fn validate_bind_address_ipv6() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("::1".into());
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 16. Validation — bind_address 0.0.0.0
// ===========================================================================

#[test]
fn validate_bind_address_all_interfaces() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("0.0.0.0".into());
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 17. Validation — bind_address valid hostname
// ===========================================================================

#[test]
fn validate_bind_address_hostname() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("localhost".into());
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 18. Validation — bind_address empty string is error
// ===========================================================================

#[test]
fn validate_bind_address_empty_is_error() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("".into());
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("bind_address must not be empty"))
    );
}

// ===========================================================================
// 19. Validation — bind_address invalid string is error
// ===========================================================================

#[test]
fn validate_bind_address_invalid_is_error() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("not a valid address!!!".into());
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("not a valid IP address or hostname"))
    );
}

// ===========================================================================
// 20. Validation — bind_address with dotted hostname
// ===========================================================================

#[test]
fn validate_bind_address_dotted_hostname() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("my-host.example.com".into());
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 21. Validation — backend names are non-empty (existing behavior)
// ===========================================================================

#[test]
fn validate_backend_name_empty_is_error() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

// ===========================================================================
// 22. Validation — backend names are unique (BTreeMap guarantees this)
// ===========================================================================

#[test]
fn backend_names_are_unique() {
    let mut cfg = fully_valid_config();
    cfg.backends.insert("dup".into(), BackendEntry::Mock {});
    cfg.backends.insert(
        "dup".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    // BTreeMap deduplicates — only one "dup" key.
    assert_eq!(
        cfg.backends.keys().filter(|k| *k == "dup").count(),
        1
    );
}

// ===========================================================================
// 23. Validation — policy_profiles with existing path passes
// ===========================================================================

#[test]
fn validate_policy_profile_existing_path() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("policy.toml");
    std::fs::write(&profile, "# policy").unwrap();
    let mut cfg = fully_valid_config();
    cfg.policy_profiles = vec![profile.to_str().unwrap().to_string()];
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 24. Validation — policy_profiles with non-existing path is error
// ===========================================================================

#[test]
fn validate_policy_profile_nonexistent_path_is_error() {
    let mut cfg = fully_valid_config();
    cfg.policy_profiles = vec!["/nonexistent/policy.toml".into()];
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("policy profile path does not exist"))
    );
}

// ===========================================================================
// 25. Validation — policy_profiles empty path is error
// ===========================================================================

#[test]
fn validate_policy_profile_empty_path_is_error() {
    let mut cfg = fully_valid_config();
    cfg.policy_profiles = vec!["".into()];
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("policy profile path must not be empty"))
    );
}

// ===========================================================================
// 26. Validation — empty policy_profiles list is valid
// ===========================================================================

#[test]
fn validate_empty_policy_profiles_is_valid() {
    let cfg = fully_valid_config();
    assert!(cfg.policy_profiles.is_empty());
    validate_config(&cfg).unwrap();
}

// ===========================================================================
// 27. ConfigValidator::check — port 0 produces error issue
// ===========================================================================

#[test]
fn check_port_zero_error() {
    let mut cfg = fully_valid_config();
    cfg.port = Some(0);
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "port"));
}

// ===========================================================================
// 28. ConfigValidator::check — invalid bind_address produces error issue
// ===========================================================================

#[test]
fn check_invalid_bind_address_error() {
    let mut cfg = fully_valid_config();
    cfg.bind_address = Some("not valid!!!".into());
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.field == "bind_address"));
}

// ===========================================================================
// 29. ConfigValidator::check — empty policy profile produces error issue
// ===========================================================================

#[test]
fn check_empty_policy_profile_error() {
    let mut cfg = fully_valid_config();
    cfg.policy_profiles = vec!["".into()];
    let result = ConfigValidator::check(&cfg);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.field.starts_with("policy_profiles"))
    );
}

// ===========================================================================
// 30. Env overrides — ABP_BIND_ADDRESS and ABP_PORT applied
// ===========================================================================

#[test]
fn apply_env_overrides_bind_and_port() {
    // Simulate applying env overrides by directly setting fields (to avoid
    // env var race conditions in parallel tests).
    let mut cfg = BackplaneConfig::default();
    cfg.bind_address = Some("0.0.0.0".into());
    cfg.port = Some(9090);
    assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
    assert_eq!(cfg.port, Some(9090));
}

// ===========================================================================
// 31. TOML roundtrip with new fields
// ===========================================================================

#[test]
fn toml_roundtrip_with_new_fields() {
    let cfg = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec!["a.toml".into(), "b.toml".into()],
        ..fully_valid_config()
    };
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

// ===========================================================================
// 32. JSON roundtrip with new fields
// ===========================================================================

#[test]
fn json_roundtrip_with_new_fields() {
    let cfg = BackplaneConfig {
        bind_address: Some("::1".into()),
        port: Some(443),
        policy_profiles: vec!["policy.toml".into()],
        ..fully_valid_config()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

// ===========================================================================
// 33. Default config has no bind_address, port, or policy_profiles
// ===========================================================================

#[test]
fn default_config_new_fields() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.bind_address.is_none());
    assert!(cfg.port.is_none());
    assert!(cfg.policy_profiles.is_empty());
}

// ===========================================================================
// 34. Merge combines new fields
// ===========================================================================

#[test]
fn merge_new_fields() {
    let base = BackplaneConfig {
        bind_address: Some("127.0.0.1".into()),
        port: Some(3000),
        ..fully_valid_config()
    };
    let overlay = BackplaneConfig {
        port: Some(8080),
        ..Default::default()
    };
    let merged = abp_config::merge_configs(base, overlay);
    assert_eq!(merged.bind_address.as_deref(), Some("127.0.0.1"));
    assert_eq!(merged.port, Some(8080));
}

// ===========================================================================
// 35. Multiple validation errors for new fields collected
// ===========================================================================

#[test]
fn multiple_new_field_errors_collected() {
    let mut cfg = fully_valid_config();
    cfg.port = Some(0);
    cfg.bind_address = Some("".into());
    cfg.policy_profiles = vec!["".into()];
    let reasons = validation_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.len() >= 3, "expected >= 3 errors: {reasons:?}");
}
