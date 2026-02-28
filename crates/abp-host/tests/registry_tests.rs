// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for `SidecarRegistry`.

use abp_host::registry::SidecarRegistry;
use abp_host::SidecarSpec;
use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// 1. Register and retrieve
// ---------------------------------------------------------------------------

#[test]
fn register_and_retrieve() {
    let mut reg = SidecarRegistry::default();
    reg.register("node", SidecarSpec::new("node"));
    let spec = reg.get("node").expect("should find registered sidecar");
    assert_eq!(spec.command, "node");
}

// ---------------------------------------------------------------------------
// 2. Register duplicate names — last write wins
// ---------------------------------------------------------------------------

#[test]
fn register_duplicate_names_overwrites() {
    let mut reg = SidecarRegistry::default();
    reg.register("node", SidecarSpec::new("node-v16"));
    reg.register("node", SidecarSpec::new("node-v20"));
    let spec = reg.get("node").expect("should find registered sidecar");
    assert_eq!(spec.command, "node-v20", "second register should overwrite first");
    assert_eq!(reg.list().len(), 1, "should still have exactly one entry");
}

// ---------------------------------------------------------------------------
// 3. Lookup by name
// ---------------------------------------------------------------------------

#[test]
fn lookup_by_name_returns_correct_spec() {
    let mut reg = SidecarRegistry::default();
    let mut spec = SidecarSpec::new("python3");
    spec.args = vec!["host.py".into()];
    spec.env.insert("DEBUG".into(), "1".into());
    reg.register("python", spec);

    let found = reg.get("python").expect("should find python");
    assert_eq!(found.command, "python3");
    assert_eq!(found.args, vec!["host.py"]);
    assert_eq!(found.env["DEBUG"], "1");
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
    reg.register("node", SidecarSpec::new("node"));
    reg.remove("node");
    assert!(reg.get("node").is_none());
}

// ---------------------------------------------------------------------------
// 5. List all sidecars (sorted)
// ---------------------------------------------------------------------------

#[test]
fn list_sidecars() {
    let mut reg = SidecarRegistry::default();
    reg.register("python", SidecarSpec::new("python"));
    reg.register("node", SidecarSpec::new("node"));
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

    let reg = SidecarRegistry::discover_from_dir(&hosts).expect("discover should succeed");
    let names = reg.list();

    // The hosts/ directory should contain at least `node` and `python`.
    assert!(
        names.contains(&"node"),
        "expected 'node' in discovered sidecars: {names:?}"
    );
    assert!(
        names.contains(&"python"),
        "expected 'python' in discovered sidecars: {names:?}"
    );

    // Each discovered spec should reference the correct interpreter.
    let node_spec = reg.get("node").unwrap();
    assert_eq!(node_spec.command, "node");
    let py_spec = reg.get("python").unwrap();
    assert_eq!(py_spec.command, "python");
}

#[test]
fn discover_from_temp_dir_with_mock_scripts() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    // Create subdirectories with recognised host scripts.
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

    let reg = SidecarRegistry::discover_from_dir(tmp.path()).expect("discover should succeed");
    let names = reg.list();

    assert_eq!(names.len(), 2, "should discover exactly 2 sidecars: {names:?}");
    assert!(names.contains(&"my-node"));
    assert!(names.contains(&"my-python"));

    let node_spec = reg.get("my-node").unwrap();
    assert_eq!(node_spec.command, "node");
    assert!(node_spec.args[0].contains("host.js"));

    let py_spec = reg.get("my-python").unwrap();
    assert_eq!(py_spec.command, "python");
    assert!(py_spec.args[0].contains("host.py"));
}

#[test]
fn discover_nonexistent_dir_is_err() {
    let result = SidecarRegistry::discover_from_dir(Path::new("/no/such/path"));
    assert!(result.is_err());
}

#[test]
fn discover_from_empty_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let reg = SidecarRegistry::discover_from_dir(tmp.path()).expect("discover should succeed");
    assert!(reg.list().is_empty(), "empty dir should produce empty registry");
}

// ---------------------------------------------------------------------------
// 7. Remove sidecar
// ---------------------------------------------------------------------------

#[test]
fn remove_sidecar() {
    let mut reg = SidecarRegistry::default();
    reg.register("node", SidecarSpec::new("node"));
    let removed = reg.remove("node");
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().command, "node");
    assert!(reg.get("node").is_none());
    assert!(reg.remove("nonexistent").is_none());
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
// 9. Serde roundtrip of SidecarSpec
// ---------------------------------------------------------------------------

#[test]
fn sidecar_spec_serde_roundtrip_minimal() {
    let spec = SidecarSpec::new("node");
    let json = serde_json::to_string(&spec).expect("serialize");
    let de: SidecarSpec = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(de.command, "node");
    assert!(de.args.is_empty());
    assert!(de.env.is_empty());
    assert!(de.cwd.is_none());
}

#[test]
fn sidecar_spec_serde_roundtrip_full() {
    let mut env = BTreeMap::new();
    env.insert("KEY".into(), "value".into());
    let spec = SidecarSpec {
        command: "python3".into(),
        args: vec!["host.py".into(), "--verbose".into()],
        env,
        cwd: Some("/workspace".into()),
    };

    let json = serde_json::to_string(&spec).expect("serialize");
    let de: SidecarSpec = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(de.command, "python3");
    assert_eq!(de.args, vec!["host.py", "--verbose"]);
    assert_eq!(de.env["KEY"], "value");
    assert_eq!(de.cwd.as_deref(), Some("/workspace"));
}

// ---------------------------------------------------------------------------
// 10. Case sensitivity of names
// ---------------------------------------------------------------------------

#[test]
fn name_lookup_is_case_sensitive() {
    let mut reg = SidecarRegistry::default();
    reg.register("Node", SidecarSpec::new("node-upper"));
    reg.register("node", SidecarSpec::new("node-lower"));

    assert_eq!(reg.get("Node").unwrap().command, "node-upper");
    assert_eq!(reg.get("node").unwrap().command, "node-lower");
    assert!(reg.get("NODE").is_none());
    assert_eq!(reg.list().len(), 2);
}

// ---------------------------------------------------------------------------
// 11. Discovery prioritises first matching script
// ---------------------------------------------------------------------------

#[test]
fn discover_prioritises_host_js_over_host_py() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    let both_dir = tmp.path().join("both");
    std::fs::create_dir(&both_dir).unwrap();
    std::fs::write(both_dir.join("host.js"), "// js").unwrap();
    std::fs::write(both_dir.join("host.py"), "# py").unwrap();

    let reg = SidecarRegistry::discover_from_dir(tmp.path()).expect("discover");
    let spec = reg.get("both").expect("should discover 'both'");
    // KNOWN_HOSTS lists host.js before host.py, so node should win.
    assert_eq!(spec.command, "node");
}

// ---------------------------------------------------------------------------
// 12. Register many sidecars and list
// ---------------------------------------------------------------------------

#[test]
fn register_many_sidecars() {
    let mut reg = SidecarRegistry::default();
    for i in 0..50 {
        reg.register(format!("sidecar-{i:03}"), SidecarSpec::new(format!("cmd-{i}")));
    }
    let names = reg.list();
    assert_eq!(names.len(), 50);
    // Sorted order check.
    assert_eq!(names[0], "sidecar-000");
    assert_eq!(names[49], "sidecar-049");
}
