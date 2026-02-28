// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_cli::config::{load_config, validate_config, BackplaneConfig};

#[test]
fn parse_valid_toml_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(
        &path,
        r#"
[backends.mock]
type = "mock"

[backends.openai]
type = "sidecar"
command = "node"
args = ["sidecar.js"]
"#,
    )
    .unwrap();

    let config = load_config(&path).unwrap();
    assert_eq!(config.backends.len(), 2);
    validate_config(&config).unwrap();
}

#[test]
fn parse_empty_config_uses_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(&path, "").unwrap();

    let config = load_config(&path).unwrap();
    assert!(config.backends.is_empty());
    validate_config(&config).unwrap();
}

#[test]
fn invalid_toml_gives_helpful_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(&path, "not valid [[[ toml").unwrap();

    let err = load_config(&path).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("failed to parse"),
        "unexpected error: {msg}"
    );
}

#[test]
fn unknown_keys_are_tolerated() {
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

    let config: BackplaneConfig = load_config(&path).unwrap();
    assert_eq!(config.backends.len(), 1);
}
