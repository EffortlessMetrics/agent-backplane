// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for `SidecarRegistry` and `SidecarConfig`.

use abp_host::registry::{SidecarConfig, SidecarRegistry};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// 1. Register and retrieve
// ---------------------------------------------------------------------------

#[test]
fn register_and_retrieve() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    let cfg = reg.get("node").expect("should find registered sidecar");
    assert_eq!(cfg.command, "node");
    assert_eq!(cfg.name, "node");
}

// ---------------------------------------------------------------------------
// 2. Duplicate registration error
// ---------------------------------------------------------------------------

#[test]
fn duplicate_registration_is_error() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node-v16"))
        .unwrap();
    let err = reg
        .register(SidecarConfig::new("node", "node-v20"))
        .unwrap_err();
    assert!(
        err.to_string().contains("already registered"),
        "expected duplicate error, got: {err}"
    );
    // Original entry is preserved.
    assert_eq!(reg.get("node").unwrap().command, "node-v16");
    assert_eq!(reg.list().len(), 1);
}

// ---------------------------------------------------------------------------
// 3. Lookup by name
// ---------------------------------------------------------------------------

#[test]
fn lookup_by_name_returns_correct_config() {
    let mut reg = SidecarRegistry::default();
    let mut cfg = SidecarConfig::new("python", "python3");
    cfg.args = vec!["host.py".into()];
    cfg.env.insert("DEBUG".into(), "1".into());
    cfg.working_dir = Some(PathBuf::from("/workspace"));
    reg.register(cfg).unwrap();

    let found = reg.get("python").expect("should find python");
    assert_eq!(found.command, "python3");
    assert_eq!(found.args, vec!["host.py"]);
    assert_eq!(found.env["DEBUG"], "1");
    assert_eq!(found.working_dir.as_deref(), Some(Path::new("/workspace")));
}

// ---------------------------------------------------------------------------
// 4. Lookup nonexistent
// ---------------------------------------------------------------------------

#[test]
fn lookup_nonexistent_returns_none() {
    let reg = SidecarRegistry::default();
    assert!(reg.get("does-not-exist").is_none());
}

#[test]
fn lookup_nonexistent_after_removal() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    reg.remove("node");
    assert!(reg.get("node").is_none());
}

// ---------------------------------------------------------------------------
// 5. List all sidecars (sorted)
// ---------------------------------------------------------------------------

#[test]
fn list_sidecars() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("python", "python"))
        .unwrap();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    // BTreeMap keeps sorted order.
    assert_eq!(reg.list(), vec!["node", "python"]);
}

// ---------------------------------------------------------------------------
// 6. Discovery from directory (temp dir with mock scripts)
// ---------------------------------------------------------------------------

#[test]
fn discover_from_hosts_dir() {
    let hosts = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("hosts");

    if !hosts.is_dir() {
        eprintln!("hosts/ directory not found, skipping discover test");
        return;
    }

    let reg = SidecarRegistry::from_config_dir(&hosts).expect("discover should succeed");
    let names = reg.list();

    assert!(
        names.contains(&"node"),
        "expected 'node' in discovered sidecars: {names:?}"
    );
    assert!(
        names.contains(&"python"),
        "expected 'python' in discovered sidecars: {names:?}"
    );

    let node_cfg = reg.get("node").unwrap();
    assert_eq!(node_cfg.command, "node");
    let py_cfg = reg.get("python").unwrap();
    assert_eq!(py_cfg.command, "python");
}

#[test]
fn discover_from_temp_dir_with_mock_scripts() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    let node_dir = tmp.path().join("my-node");
    std::fs::create_dir(&node_dir).unwrap();
    std::fs::write(node_dir.join("host.js"), "// mock").unwrap();

    let py_dir = tmp.path().join("my-python");
    std::fs::create_dir(&py_dir).unwrap();
    std::fs::write(py_dir.join("host.py"), "# mock").unwrap();

    // A directory without a recognised script should be ignored.
    let empty_dir = tmp.path().join("ignored");
    std::fs::create_dir(&empty_dir).unwrap();
    std::fs::write(empty_dir.join("README.md"), "nothing").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).expect("discover should succeed");
    let names = reg.list();

    assert_eq!(
        names.len(),
        2,
        "should discover exactly 2 sidecars: {names:?}"
    );
    assert!(names.contains(&"my-node"));
    assert!(names.contains(&"my-python"));

    let node_cfg = reg.get("my-node").unwrap();
    assert_eq!(node_cfg.command, "node");
    assert!(node_cfg.args[0].contains("host.js"));

    let py_cfg = reg.get("my-python").unwrap();
    assert_eq!(py_cfg.command, "python");
    assert!(py_cfg.args[0].contains("host.py"));
}

#[test]
fn discover_nonexistent_dir_is_err() {
    let result = SidecarRegistry::from_config_dir(Path::new("/no/such/path"));
    assert!(result.is_err());
}

#[test]
fn discover_from_empty_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let reg = SidecarRegistry::from_config_dir(tmp.path()).expect("discover should succeed");
    assert!(
        reg.list().is_empty(),
        "empty dir should produce empty registry"
    );
}

// ---------------------------------------------------------------------------
// 7. Remove sidecar
// ---------------------------------------------------------------------------

#[test]
fn remove_sidecar() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    assert!(reg.remove("node"));
    assert!(reg.get("node").is_none());
    assert!(!reg.remove("nonexistent"));
}

// ---------------------------------------------------------------------------
// 8. Empty registry
// ---------------------------------------------------------------------------

#[test]
fn empty_registry_has_no_entries() {
    let reg = SidecarRegistry::default();
    assert!(reg.list().is_empty());
    assert!(reg.get("anything").is_none());
}

// ---------------------------------------------------------------------------
// 9. Serde roundtrip of SidecarConfig
// ---------------------------------------------------------------------------

#[test]
fn sidecar_config_serde_roundtrip_minimal() {
    let cfg = SidecarConfig::new("node", "node");
    let json = serde_json::to_string(&cfg).expect("serialize");
    let de: SidecarConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(de.name, "node");
    assert_eq!(de.command, "node");
    assert!(de.args.is_empty());
    assert!(de.env.is_empty());
    assert!(de.working_dir.is_none());
}

#[test]
fn sidecar_config_serde_roundtrip_full() {
    let mut env = BTreeMap::new();
    env.insert("KEY".into(), "value".into());
    let cfg = SidecarConfig {
        name: "python".into(),
        command: "python3".into(),
        args: vec!["host.py".into(), "--verbose".into()],
        env,
        working_dir: Some(PathBuf::from("/workspace")),
    };

    let json = serde_json::to_string(&cfg).expect("serialize");
    let de: SidecarConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(de.name, "python");
    assert_eq!(de.command, "python3");
    assert_eq!(de.args, vec!["host.py", "--verbose"]);
    assert_eq!(de.env["KEY"], "value");
    assert_eq!(de.working_dir.as_deref(), Some(Path::new("/workspace")));
}

// ---------------------------------------------------------------------------
// 10. Config validation — missing command
// ---------------------------------------------------------------------------

#[test]
fn validate_empty_command_is_error() {
    let cfg = SidecarConfig::new("node", "");
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("command"),
        "expected command error, got: {err}"
    );
}

#[test]
fn validate_empty_name_is_error() {
    let cfg = SidecarConfig::new("", "node");
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("name"),
        "expected name error, got: {err}"
    );
}

#[test]
fn register_invalid_config_is_error() {
    let mut reg = SidecarRegistry::default();
    let err = reg.register(SidecarConfig::new("node", "")).unwrap_err();
    assert!(
        err.to_string().contains("command"),
        "expected validation error, got: {err}"
    );
    assert!(reg.list().is_empty(), "invalid config should not be stored");
}

// ---------------------------------------------------------------------------
// 11. Case sensitivity of names
// ---------------------------------------------------------------------------

#[test]
fn name_lookup_is_case_sensitive() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("Node", "node-upper"))
        .unwrap();
    reg.register(SidecarConfig::new("node", "node-lower"))
        .unwrap();

    assert_eq!(reg.get("Node").unwrap().command, "node-upper");
    assert_eq!(reg.get("node").unwrap().command, "node-lower");
    assert!(reg.get("NODE").is_none());
    assert_eq!(reg.list().len(), 2);
}

// ---------------------------------------------------------------------------
// 12. Discovery prioritises first matching script
// ---------------------------------------------------------------------------

#[test]
fn discover_prioritises_host_js_over_host_py() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    let both_dir = tmp.path().join("both");
    std::fs::create_dir(&both_dir).unwrap();
    std::fs::write(both_dir.join("host.js"), "// js").unwrap();
    std::fs::write(both_dir.join("host.py"), "# py").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).expect("discover");
    let cfg = reg.get("both").expect("should discover 'both'");
    // KNOWN_HOSTS lists host.js before host.py, so node should win.
    assert_eq!(cfg.command, "node");
}

// ---------------------------------------------------------------------------
// 13. Register many sidecars and list
// ---------------------------------------------------------------------------

#[test]
fn register_many_sidecars() {
    let mut reg = SidecarRegistry::default();
    for i in 0..50 {
        reg.register(SidecarConfig::new(
            format!("sidecar-{i:03}"),
            format!("cmd-{i}"),
        ))
        .unwrap();
    }
    let names = reg.list();
    assert_eq!(names.len(), 50);
    assert_eq!(names[0], "sidecar-000");
    assert_eq!(names[49], "sidecar-049");
}

// ---------------------------------------------------------------------------
// 14. Config to_spec conversion
// ---------------------------------------------------------------------------

#[test]
fn config_to_spec_conversion() {
    let mut cfg = SidecarConfig::new("my-node", "node");
    cfg.args = vec!["host.js".into()];
    cfg.env.insert("PORT".into(), "3000".into());
    cfg.working_dir = Some(PathBuf::from("/tmp/work"));

    let spec = cfg.to_spec();
    assert_eq!(spec.command, "node");
    assert_eq!(spec.args, vec!["host.js"]);
    assert_eq!(spec.env["PORT"], "3000");
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/work"));
}

#[test]
fn config_to_spec_no_working_dir() {
    let cfg = SidecarConfig::new("simple", "bash");
    let spec = cfg.to_spec();
    assert_eq!(spec.command, "bash");
    assert!(spec.args.is_empty());
    assert!(spec.cwd.is_none());
}

// ---------------------------------------------------------------------------
// 15. Discovered configs have correct name field
// ---------------------------------------------------------------------------

#[test]
fn discovered_configs_have_correct_name() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    let dir = tmp.path().join("my-sidecar");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("host.py"), "# mock").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    let cfg = reg.get("my-sidecar").unwrap();
    assert_eq!(cfg.name, "my-sidecar");
    assert_eq!(cfg.command, "python");
}

// ---------------------------------------------------------------------------
// 16. discover_from_dir alias works
// ---------------------------------------------------------------------------

#[test]
fn discover_from_dir_alias_works() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let dir = tmp.path().join("node-sc");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("host.js"), "// alias").unwrap();

    let reg = SidecarRegistry::discover_from_dir(tmp.path()).unwrap();
    assert_eq!(reg.list(), vec!["node-sc"]);
}

// ---------------------------------------------------------------------------
// 17. Remove returns false for unknown name
// ---------------------------------------------------------------------------

#[test]
fn remove_unknown_returns_false() {
    let mut reg = SidecarRegistry::default();
    assert!(!reg.remove("ghost"));
}

// ---------------------------------------------------------------------------
// 18. Register after remove re-allows the name
// ---------------------------------------------------------------------------

#[test]
fn register_after_remove_succeeds() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("x", "cmd1")).unwrap();
    reg.remove("x");
    reg.register(SidecarConfig::new("x", "cmd2")).unwrap();
    assert_eq!(reg.get("x").unwrap().command, "cmd2");
}

// ---------------------------------------------------------------------------
// 19. SidecarConfig defaults via serde
// ---------------------------------------------------------------------------

#[test]
fn sidecar_config_deserialize_defaults() {
    let json = r#"{"name":"n","command":"c"}"#;
    let cfg: SidecarConfig = serde_json::from_str(json).expect("deserialize");
    assert_eq!(cfg.name, "n");
    assert_eq!(cfg.command, "c");
    assert!(cfg.args.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.working_dir.is_none());
}

// ---------------------------------------------------------------------------
// 20. Validate both name and command empty
// ---------------------------------------------------------------------------

#[test]
fn validate_both_empty_reports_name() {
    let cfg = SidecarConfig::new("", "");
    let err = cfg.validate().unwrap_err();
    // Name is checked first.
    assert!(
        err.to_string().contains("name"),
        "expected name error first, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 21. List after clear
// ---------------------------------------------------------------------------

#[test]
fn list_after_removing_all() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("a", "a")).unwrap();
    reg.register(SidecarConfig::new("b", "b")).unwrap();
    reg.remove("a");
    reg.remove("b");
    assert!(reg.list().is_empty());
}
